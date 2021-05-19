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
        "rad_metadata": {
            "automerge_type": "text"
        }
    },
    "description": {
        "type": "string"
        "rad_metadata": {
            "automerge_type": "text"
        }
    },
    "author": {
        "type": "string",
        "description": "The radicle ID of the author of the issue"
        "rad_metadata": {
            "automerge_type": "text"
        }
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
                    "rad_metadata": {
                        "automerge_type": "text"
                    }
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
respective author. This is an extremely simplified model. Note the presence of
the `rad_metadata` key on some schema items, more on this later.

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

Despite all the complexity under the hood, the API of automerge is relatively
simple. Automerge works in terms of "documents", a document is a single log of
changes. Every time you modify an automerge document you generate a new entry 
for the change log. Each change is just some bytes. When you receive changes 
from other actors you just pass these changes (which, again, are just bytes) to
automerge to add to the change log. The end result is that you load a bunch of
binary changes and get back a JSON object.

There are some subtleties around preserving user intent when modifying
documents, but these are not too onerous.

### Object Trees

Objects are represented by a tree with the following layout:

```
.
|--manifest
|--schema
|  |--schema.json
|  |--migrations
|  |  |--<migration hash>.json
|  |  |--<migration hash>.json
|--history
|  |--<change 1 hash>
|  |--<change 2 hash>
```

This tree contains the state of a single collaborative object. We will go into
more details shortly. However, first let us discuss how they are represented
and transported.

We refer to the tuple `(manifest, (shchema.json, [schema changes]), [changes])`
as an "object state".

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
initial schema. Schema migrations will not be addressed in detail in this RFC
but we will show their feasibility.

### Mapping the Automerge hash graph to Git

TODO

### Fetching Collaborative Objects

Each time a repository creates a collaborative object tree it creates a ref
pointing to that object at `refs/namespaces/<project>/rad/collaborative-objects/<typename>/<object ID>`, 
where `object ID` is a unique identifier generated at creation time. 

This allows us to fetch subsets of collaborative objects by specifying refspecs
that match them. The downside is that we are adding a ref to the initial
advertised refs, each of theses refs is around 250 bytes. If we consider a popular
repository such as https://github.com/facebook/react/ you can see that they have
nearly 10,000 issues (including both Open and Closed, which we must). That would 
mean that the initial ref advertisment in any replication of this repository
would be ~2.5Mb. If we could use v2 of the git pack protocol this problem would
go away but alas, we must use v1. 

To get around the problem we can add a parameter to the radicle URL which
indicates either a single object ID or a type name which we wish to fetch
from the remote, this would then be passed as a custom header to the transport
and interpreted on the server. In this manner we can allow clients to choose
when they want to replicate collaborative objects, which would allow staged
fetches where we first fetch the repository and identities, and then fetch 
collaborative objects.

Therefore we add two parameters to a radicle URN:

- `collab_object_id`: specifies that a server should only consider references to the given object ID
- `collab_type_name`: specifies that a server should only consider references to the given type name

These parameters are emitted by the radicle p2p transport as headers in a 
similar fashion to the `n=` parameter for the nonce or the `ls=true`
parameter for selecting the `upload-pack-ls` service. The Radicle git server
can then use these parameters to filter the refs it operates on.

### Viewing the tracking graph

Assuming we have replicated a number of collaborative objects from our tracking
graph, we can now view the merged state of those objects. To do this we search
through every `/rad/collaborative-objects/<typename>/<object ID>` reference for
every remote we have and collect the change files for each object ID, then we
merge all of these changes using Automerge. This is subject to some additional
logic regarding schemas which is outlined later.

It is important to note that this merging is at this point not stored in the
repository - it can be performed in memory and may be cached. The result is
that the user sees a single merged view of the object based on the contents
of the remotes they have replicated. That is, there is no additional
merge-then-commit step.

### Updating objects

To make a change to an object we load the existing messages for an object. The 
application developer provides us with the binary representation of the change
to that object. We apply the change and ensure that the new object state still
matches the object schema. At this point the state of the object may depend on
many contributions from the tracking graph - not just the ones in our own view
of the project. We ensure that the new state matches the schema and then


### Strange Perspectives

This model introduces some counter-intuitive properties. For example, I might
"create an issue" in a repository and anyone who is tracking me would see that
issue, but people who are tracking the project but don't have me in their
tracking graph will only see the issue if the maintainer replies to it. It's
hard to see how you would do things like "link to an issue" under these
constraints. This is inherent to the network model though, rather than being a
specific problem of this architecture.

We can work around some of this weirdness using seed nodes. If we consider
seed nodes 

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
- create a new object from a JSON object, a JSON schema, and a type name
  
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

### JSON Patch instead of Automerge

TODO
