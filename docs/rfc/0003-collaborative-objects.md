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
- Interoperable. There should be a straightforward API for disparate tools to
  interact with collaborative objects

Local first because you should not need to be online to modify collaborative
objects and no one should be able to stop you from changing data that is on
your computer. That said, collaborative objects are _collaborative_, users need
to be able to receive changes to the same data from other people and this
raises the problem of how to merge those changes. I see two options here:

- CRDTs, data structures which allow conflict free merges
- Application level merges. Much as Git requires user action to merge
  conflicting changes, we could expose the merge points of collaborative
  objects to the user to resolve.

The latter here is undesirable for a lot of reasons but most relevant is that
it conflicts with extensibility and interoperability. in order to provide a good 
UX tools would need to provide a UI for users to do a three way merge of the 
data they are working on. This contradicts the interoperability goal.

This leaves us with CRDTs. For the purposes of this RFC I am going to assume
that we will use the [Automerge](https://github.com/automerge/automerge) CRDT
implementation. Automerge is capable of merging arbitrary JSON objects (the
data model is actually richer than JSON, with types for byte arrays, various
precision integers and floats etc.). An alternative approach would be to write
a custom CRDT for each data type we want to replicate, see [##Alternative
Approaches] for a discussion of this design.

## Example: Issues

To motivate and contextualise this proposal we introduce a running example
which is dear to many peoples hearts; issues. We imagine this from the
perspective of the application developer. For expositional purposes we assume
that the application communicates with a radicle implementation via an HTTP
RPC. However, the HTTP RPC is not a proposal of this RFC, the proposed API will
be specified in terms of `librad` later in this document.

The first thing the developer must do is decide on the schema of their data and
represent it as a JSON schema. We use this simple schema:

```json
{
  "type": "object"
  "properties": {
    "title": {
        "type": "string"
    },
    "description": {
        "type": "string"
    },
    "author": {
        "type": "string",
        "description": "The radicle ID of the author of the issue"
    },
    "signature": {
        "type": "string",
        "description": "A base64 encoded signature of the issue"
    },
    "comments": {
        "type": "array",
        "items": {
            "type": "object",
            "properties": {
                "text": {"type": "string"},
                "author": {
                    "type": "string",
                    "description": "Radicle ID of the author of the comment"
                },
                "signature": {
                    "type": "string",
                    "description": "Base64 encoded signature of the comment"
                }
            }
        }
    }
  }
}
```

An issue consists of a title, description, and author along with the author's
signature; followed by a list of comments, each of which is signed by it's
respective author. This is an extremely simplified model. 

This schema may well be the subject of it's own mini standardisation process
as it is very likely that many different applications will want to interoperate
with the same issue model. The important thing is that this standardisation
process can happen independently of the radicle protocol.

In addition to the schema, the developer must choose a name for their type.
This is similar to an XML namespace and probably standardised as part of the
same process which produces the schema. In this case let's choose
`https://radicle.xyz/issue` as the type name.


### Creating an issue

The first thing a user will wish to do is to create a new issue. To do
this they make a POST request to `<radicle implementation>/projects/metadata`
with the following content:

```json
{
    "typename": "https://radicle.xyz/issue",
    "schema": <the schema above>,
    "data": {
        "title": "Librad doesn't implement the metadata RFC",
        "description": "It's in the name",
        "author": "<some base64>",
        "signature": "<some base64>",
        "comments": [],
    }
}
```

This endpoint returns an error if the data does not match the schema. Otherwise 
the endpoint returns an identifier for the newly created object and announces
the new data to the network, anyone tracking the project will pull those 
changes.

### Retrieving an issue

The next step then is for users to retrieve project metadata. Imagine the user
has just received the metadata posted in the previous example, we can retrieve
that data by making a request like this (url encoded of course):

```
GET <radicle implementation/projects/<project URN>/metadata?typename=https://radicle.xyz/issue
```

This will return something like this:

```
[
    {
        "id": "<some long string>",
        "typename": "https://radicle.xyz/issue",
        "schema": <the schema above>,
        "data": {
            "title": "Librad doesn't implement the metadata RFC",
            "description": "It's in the name",
            "author": "<some base64>",
            "signature": "<some base64>",
            "comments": [],
        },
        "history": {
            "type": "automerge",
            "changes": "<some base64>"
        }
    }
]
```

This mysterious `history` key will be explained next.

### Adding a comment

Up to this point this has been a straightforward ReST API, it is at the point
that we wish to make changes that the distributed nature of the data structure
intrudes. We cannot directly mutate the data, instead we need to create a 
change which describes how we want to update the data - this change includes
metadata which allows other people to incorporate that change into their
version of the data at any time. In this case we use the automerge Javascript
API to do this. That would look like the following:

```typescript
import * as Automerge from "automerge"

const data = await fetch("<metadata URL>").then(r => r.json())
const doc = Automerge.load(base64ToUint8(data.history.changes))
const updatedDoc = Automerge.change(doc, d => {
    d.comments.push({
        "text": "I completely agree!",
        "author": "<some base64>",
        "signature": "<some base64>"
    })
})
const change = Automerge.getChanges(doc, updatedDoc)
const changeBytes = uint8ToBase64(change)
```

What we do here is load the automerge document from it's history, then use the
automerge Javscript library to mutate the document (the `Automerge.change`
call) and then finally get the change between the original version of the 
document and the new one. 

Now that we have the change we can make a `PATCH` request to 
`<radicle-implementation>/projects/<project URN>/metadata/<metadata ID>` with
the following contents:

```json
{
    "changes": {
        "type": "automerge",
        "change": "<some base64>
    }
}
```

This endpoint will return an error if the change does not match the schema of
the object. Otherwise the change will be merged in to the object and announced
to the network.


### Changing the schema

It's a few months on, everyone is very happy with issues except for one thing,
there is no way to react to a comment with an emojii. To accomodate this we
modify the schema to add a `reaction` field to the `comment` schema. Now when
we create an issue, as well as passing the schema, we also pass a schema 
migration. Like so:

```json
{
    "typename": ...,
    "schema": ...,
    "schema_migrations": [
        {
            "type": "add_field",
            "path": "comments",
            "name": "reaction",
            "schema": {
                "type": "string",
                "maxLength": 1,
            }
        }
    ],
    "data": ...
}
```

And when updating an object:

```json
{
    "changes": ...,
    "schema_migrations": <as above>
}
```

There are restrictions on these migrations. You can only ever add optional 
fields, or fields with a default value. This schema migration must be bundled
with the application, must as database migrations are bundled in the source
code of web 2.0 applications.

## Implementation

### Automerge

It is useful to briefly outline how automerge functions in order for the
following to be sensible. Everything automerge does is based on a merging a log
of operations. An operation might be something like "create a list under the
'comments' key of the root object", or "insert the character 'a' after the 
character inserted by the 2nd change actor 1 made". Every operation has an 
identifier - which allows statements like "the character inserted by the 2nd
change actor 1 made" to be precise. This operation ID is the combination of a
unique identifier for each actor, and an always incrementing sequence number.
This construction, along with sorting by actor IDs in the case of a tie, allows
us to place operations in a total order which respects causality. i.e if I add
an operation then no operation that I could have observed at the time I made
the operation will come after it in the log.

Automerge defines a number of operations along with merge semantics for those
operations. More detail on that can be found in [the implementation](https://github.com/automerge/automerge)
and in [the paper](https://arxiv.org/abs/1608.03960).

Despite all the complexity under the hood the API of automerge is relatively
simple. Automerge works in terms of "documents", a document is a single log of
changes. Every time you modify an automerge document you generate a new entry 
for the change log. Each change is just some bytes. When you receive changes 
from other actors you just pass these changes (which, again, are just bytes) to
automerge to add to the change log. The end result is that you load a bunch of
binary changes and get back a JSON object.

There are some subtleties around preserving user intent when modifying
documents, but these are not too onerous.

### Message storage and transmission

The property of CRDTs that we care about is strong eventual consistency: any
two nodes which have received the same set of messages will merge to the same
state. A major part of the design of collaborative objects therefore is how we
store and transmit these messages. Messages in this context are just binary
blobs which need to be passed to the CRDT implementation in any order.

Objects are created with a unique identifier at creation time. We use this
identifier to create a tree with the following structure:

```
.
|--manifest
|--schema
|  |--schema.json
|  |--migrations
|  |  |--<migration hash>.json
|  |  |--<migration hash>.json
|--<change 1 hash>
|--<change 2 hash>


We add a new entry to the `signed_refs` of a project at
`refs/namespaces/<project>/remotes/refs/remotes/<peer>/rad/collaborative-objects`.
This points to a tree with the following structure:

```
.
|--<type name: e.g https://radicle.xyz/issue>
|  |--<object 1 ID>
|  |  |--manifest
|  |  |--schema
|  |  |  |--schema.json
|  |  |  |--migrations
|  |  |  |  |--<migration hash>.json
|  |  |  |  |--<migration hash>.json
|  |  |--<change 1 hash>
|  |  |--<change 2 hash>
|  |--<object 2 ID>
|  |  ...
|--<another type name>
|  |--<object 3 ID>
|  |  ...
```

We refer to a directory in this tree as an "object directory" and the tuple
`(manifest, (shchema.json, [schema changes]), [changes])` as an "object state".

Objects are created with a unique identifier at creation time. This identifier
is the name of the directory within the `collaborative_objects` tree where the
data for this object is kept. Each time we make a change to the object we store
the new change (which is a binary blob) in the object directory and create a
new commit. We also store a manifest file which contains the name of the data
type of this object. This file is never changed and is used to allow
applications to enumerate collaborative objects of a particular type. Type
names should be some kind of human readable identifier - similar to XML
namespaces.

Each object is also created with a JSON schema. The schema is represented by an
initial `schema.json` and a series of schema migrations which extend that
initial schema. We will examine schema migrations shortly.

### Mapping the Automerge hash graph to Git

Much like Git, Automerge documents are a hash linked graph of changes. Each
change is like a commit and references zero or more dependencies via their
hashes. We can map this structure to git in the following manner:

Every every time we make a chang


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

## Blessed Data Types

This project metadata mechanism is extremely broad, which has a lot of upsides
but it runs the risk of running into XMPP style extension hell, where every
peer is running a different set of extensions. It might be worthwhile to bundle
a few core extensions with librad - issues for example.

## Alternative Approaches

### Domain Specific CRDTs

TODO
