# RFC: Collaborative Objects

* Author: @alexjg
* Date: 2021-05-04
* Status: draft
* Community discussion: ...

## Motivation

Existing code collaboration tools usually have metadata associated with a
project which are not source code. These are items such as issues, project
boards, pull request discussions, etc. Supporting these metadata is a key goal
of the Radicle network. However, ideally we would not be opinionated about
exactly what such metadata look like, different organisations and people will
have different requirements and one of the promises of decentralisation is to
increase user choice. Therefore we should remain agnostic at the protocol level
about exactly what such metadata looks like, instead we should build a single
API for applications to use metadata associated with a project. The schemas and
interpretations of these data types then become composable at the application
layer.

## Design Goals

- Local first
- Extensible. It should be possible to extend Radicle with new collaborative
  data types without changing the protocol version
- Interoperable. There should be a straightforward API for other tools to
  interact with collaborative objects

The local first goal matches Radicles peer to peer design and philosophy. You
should not need to be online to modify collaborative objects and no one should
be able to stop you from changing data that is on your computer. That said,
collaborative objects are _collaborative_, users need to be able to receive
changes to the same data from other people and this raises the problem of how
to merge those changes. I see two options here:

- CRDTs, data structures which allow conflict free merges
- Application level merges. Much as Git requires user action to merge
  conflicting changes, we could expose the merge points of collaborative
  objects to the user to resolve.

The latter here is undesirable for a lot of reasons but most relevant is that
it conflicts with extensibility and interoperability. If we want to add new
data types without changing the protocol version then the only general approach
for application level merges is to ask the user to directly merge the
underlying data structures. Requiring users to understand the underlying
representations of the data they are working with is awful UX which means that
in order to provide a good UX tools would need to provide a UI for users to do
a three way merge of the data they are working on. This contradicts the
interoperability goal.

This leaves us with CRDTs. For the purposes of this RFC I am going to assume
that we will use the [Automerge](https://github.com/automerge/automerge) CRDT
implementation. Automerge is capable of merging arbitrary JSON objects (the
data model is actually richer than JSON, with types for byte arrays, various
precision integers and floats etc.). An alternative approach would be to write
a custom CRDT for each data type we want to replicate, see [##Alternative
Approaches] for a discussion of this design.

## Message storage and transmission

The property of CRDTs that we care about is strong eventual consistency: any
two nodes which have received the same set of messages will merge to the same
state. A major part of the design of collaborative objects therefore is how we
store and transmit these messages. Messages in this context are just binary
blobs which need to be passed to the CRDT implementation in any order.

We add a new entry to the `signed_refs` of a project at
`refs/namespaces/<project>/remotes/refs/remotes/<peer>/rad/collaborative-objects`.
This points to a tree with the following structure:

```
.
|--<object 1 ID>
|  |--manifest
|  |--schema
|  |  |--<schema change 1>
|  |  |--<schema change 2>
|  |--<change 1>
|  |--<change 2>
|--<object 2 ID>
|  ...
```

We refer to a directory in this tree as an "object directory" and the tuple 
`(manifest, [schema changes], [changes])` as an "object state".

Objects are created with a unique identifier at creation time. This identifier
is the name of the directory within the `collaborative_objects` tree where the
data for this object is kept. Each time we make a change to the object we store
the new change (which is a binary blob) in the object directory and create a
new commit. We also store a manifest file which contains the name of the data
type of this object. This file is never changed and is used to allow
applications to enumerate collaborative objects of a particular type. Type
names should be some kind of human readable identifier - similar to XML
namespaces.

Each object is also created with a schema, this is a JSON schema document which
is itself encoded as an automerge document within the `schema` directory. This
allows for changes to the schema (as long as they are backwards and forwards
compatible) to be distributed as additional changes within the `<object
ID>/schema` directory.

### Viewing the tracking graph

We need to load the state of objects which multiple people within the
tracking graph are collaborating on. Given a particular object ID we can load
all the changes from each remote we have replicated and merge them all to
obtain the final state of the object. With many remotes this could become
expensive and there are opportunities to do some indexing ahead of time. A more
pressing concern is that we want to ensure that the schema is respected, more
on this later.


### Updating objects

To make a change to an object we load the existing messages for an object. The 
application developer provide us with the binary representation of the change
to that object. We apply the change and ensure that the new object state still
matches the object schema. At this point the state of the object may depend on
many contributions from the tracking graph - not just the ones in our own view
of the project. This is a natural point at which to compact the document using
automerge's compacted document format. We bundle all the changes into one
compacted change file and replace all the changes in our view of the object
with this compacted change file - then create a new commit, update our signed
refs, and announce.

### Strange Perspectives

This model introduces some counter-intuitive properties. For example, I might
"create an issue" in a repository and anyone who is tracking me would see that
issue, but people who are tracking the project but don't have me in their
tracking graph will only see the issue if the maintainer replies to it. It's
hard to see how you would do things like "link to an issue" under these
constraints. This is inherent to the network model though, rather than being a
specific problem of this architecture.

## Schemas

Schemas are primarily important for the interoperability of the system. We need
applications to be able to rely on the data they are working with being valid,
otherwise we impose the problem of schema validation on application developers.
We represent schemas using automerge documents which contain a JSON schema.

Representing schemas as automerge documents means that we can allow for _some_
schema migration, but we need to ensure that changes to the schemata are
backwards and forwards compatible. Loading a schema for a particular object
state looks roughly like this: 
- Load all of the schema's changes
    - Merge all of the changes to put them in causal order
    - Apply the changes in causal order checking at each change that the new
      document is a valid JSON schema and that it is forward and backwards
      compatible with the previous schema.

At this point we know we have a valid schema for each object state. We now
check that the schemata for each object state are forwards and backwards
compatible with each other, at which point we can merge all the schema changes
we have to obtain the valid schema for the object. 

In the case that an object has an incompatible schema with respect to other
objects in our tracking graph we do not want to reject the entire object as
this would provide an avenue for griefing. In the case where the current user
has a version of the object in their own repo then we can privilege objects
which are compatible with that object, in more complex cases we will need some
kind of heuristic - maybe we partition the set of objects by compatibility and
choose the largest partition, or we choose an object from some preferred set of
peers (say the project maintainers). 

This is all pretty complicated, one alternative would be to store the schemas
in the `ProjectPayload` rather than alongside the objects themselves. This
would restrict the kinds of objects that can be used with a given project to
those that have been "approved" in some manner by the maintainers and would
require an update to the project identity to add new schemas. I think ideally
we would avoid this as it also means that schema migrations would require
action from project maintainers, but it may prove necessary.

## APIs

The APIs librad will provide:

- enumerate collaborative objects of a particular type
- retrieve an object with a particular ID as a JSON representation for
  applications which only wish to read data
- retrieve an object with a particular ID as an Automerge document for
  applications which wish to write data
- update an object by providing the bytes of an automerge change which updates
  the document
- create a new object from a JSON object, an automerge document containing a 
  schema, and a type name
- update the schema for all objects of an existing type by providing an the 
  binary representation of an automerge change which modifies the existing
  schema
  
Note that I am referring to "the binary representation of an automerge x" 
because the automerge API works in terms of binary changes.


TODO: spell out what this looks like in code

### Extended Example: issues

TODO

## Blessed Data Types

This project metadata mechanism is extremely broad, which has a lot of upsides
but it runs the risk of running into XMPP style extension hell, where every
peer is running a different set of extensions. It might be worthwhile to bundle
a few core extensions with librad - issues for example.

## Alternative Approaches

### Domain Specific CRDTs

TODO
