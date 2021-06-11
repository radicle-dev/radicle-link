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
  "definitions": {
    "comment": {
        "type": "object",
        "rad_signed_by": {
          "properties": ["parent_id", "text", "author"],
          "data": {
            "keys": "0/author"
          },
        },
        "properties": {
            "text": {"type": "string"},
            "id": "string",
            "children": {
                "type": "array",
                "grow_only": true,
                "items": {
                    "$ref": "#/definitions/comment"
                }
            }
            "parent_id": {
                "oneOf": [
                    "data": {
                        "const": "1/id"
                    },
                    "const": null
                ]
            }
            "author": {
                "type": "string",
                "description": "Radicle URN of the author of the comment",
                "automerge_type": "text"
            },
        }
    }
  },
  "type": "object",
  "rad_signed_by": {
    "fields": ["title", "description", "author"],
    "keys": ["<authors URN>"]
  },
  "properties": {
    "id": {
      "type": "string",
      "frozen": true
    },
    "title": {
        "type": "string",
        "automerge_type": "text"
    },
    "description": {
        "type": "string",
        "automerge_type": "text"
    },
    "author": {
        "description": "The radicle ID of the author of the issue",
        "const": "<the authors URN>",
        "frozen": true
    },
    "comments": {
        "type": "array",
        "items": {
            "$ref": "#/definitions/comment"
        }
    }
  }
}
```

An issue consists of a title, description, and author along with the author's
signature; followed by a list of comments, each of which is signed by it's
respective author. This is an extremely simplified model. Note the presence of
the `automerge_type`, `frozen`, `grow_only`, `rad_signed_by`, and `rad_urn` keys. 

The `automerge_type` key indicates that this field should be stored as a
special data type in the automerge document which tracks individual character
inserts and deletions, this allows us to merge edits to the same piece of text
in a non surprising manner.

The `frozen` key indicates that any change which modifies this field should be
ignored.

The `grow_only` key indicates that elements may not be removed from this 
sequence. This allows us to ensure that comments cannot be removed.

The `rad_signed_by` and is more interesting. The  `rad_signed_by` field tells
librad to validate that the given properties (in this case the `title`,
`description`, and `author` properties) are signed by the given identities.
Combined with the constant URN and `frozen` on the author this allows us to
ensure that only the author of the issue can change the description or title.

Note that comments are described by a tree structure where the parent ID
uses the `"$data"` keyword of the [data JSON schema vocabulary](https://gregsdennis.github.io/json-everything/usage/vocabs-data.html)
to add the constraint that the parent ID must be the ID of the parent comment
in the document structure. This allows us to impose a partial order on comments
which cannot be rewritten by arbitrary writers. We also use the `$data` keyword
to allow comments to dynamically state what the key they should be signed by
is.

This schema may well be the subject of its own mini standardisation process
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
        "author": "<the authors URN>",
        "signatures": [
            {
                "key": "<some base32-z>",
                "signature":  "<some base32-z>" 
            }
        ],
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

Operations in automerge are transported in batches called "changes". Each 
change references zero or more changes it depends on via their hash. In this 
manner automerge is similar to git in that it's a hash linked graph of changes.

Despite all the complexity under the hood, the API of automerge is relatively
simple. Automerge works in terms of "documents", a document is a single log of
changes. Every time you modify an automerge document you generate a new entry 
for the change log. Each change is just some bytes. When you receive changes 
from other actors you just pass these changes (which, again, are just bytes) to
automerge to add to the change log. The end result is that you load a bunch of
binary changes and get back a JSON object.

There are some subtleties around preserving user intent when modifying
documents, but these are not too onerous.

### Change Commits

Given that automerge changes are a hash linked graph, we can map them to Git.
We do so by wrapping each change in a commit. The commit points at a tree with
the following layout

```
.
|--change
|  |--manifest
|  |--schema
|  |  |--schema.json
|  |  |--migrations
|  |  |  |--<migration hash>.json
|  |  |  |--<migration hash>.json
|  |--<change hash>
|--signatures
```

This tree contains a single change to a collaborative object. We will go into
more details shortly. Any direct dependencies of this change are encoded in the 
same manner and become the parents of this commit. This allows us to
reconstruct the automerge depdency graph. 

Along with the dependencies of the commit we also need to add the commit of the
identity which created this commit. We need this identity to validate
signatures and by making the commit a parent we ensure that git will replicate
it for us. 

A valid change commit must have two trailers:

- `X-Rad-Signature`, as for identity documents
- `X-Rad-Author-Parent`, this is the hash of the commit which references the
  author identity. We use this trailer to avoid following the author commit
  reference when constructing the automerge change graph


#### `change/Manifest`

The manifest is a TOML file containing some metadata about the object.
Specifically it will contain:

- `id`, a UUID, generated at the time the object is created
- `typename`, discussed above
- `history_type`, always `"automerge"`, this is here to allow for different
  CRDT implementations in future.


Each object is also created with a JSON schema. The schema is represented by an
initial `schema.json` and a series of schema migrations which extend that
initial schema. Schema migrations will not be addressed in detail in this RFC
but we will show their feasibility.

#### `change/schema`

Schemas are primarily important for the interoperability of the system. We need
applications to be able to rely on the data they are working with being valid,
otherwise we impose the problem of schema validation on application developers.
We represent schemas using JSON schema which is in the `schema/schema.json` of any
object tree.

Schemas will need to be able to change, the `schema/migrations` directory is
present to allow us to store compatible changes to a schema in future. Schema
migration is out of scope for this RFC.


#### `schema/<change hash>`

This is the automerge change which this commit introduces. It is a binary file
which must contain a single change and it's dependents must be the dependents
referenced by the parents of the commit.


#### `author`

This is a file containing the SHA of the commit which references the authors
identity. We need this so we know to ignore this commit when walking the
history to look for changes.

#### `signatures`

This is the signature of the `change` tree using the key in `author`. This uses
the same format as that of `X-Rad-Signature` trailer on commit messages.

### Reconstructing Collaborative Objects

Assuming we have replicated a number of collaborative objects from our tracking
graph, we can now view the merged state of those objects. To do this we search
through every `/rad/collaborative-objects/<typename>/<object ID>` reference for
every remote we have and collect the change files for each object ID.

At this point we have the hash linked graph of automerge changes, but we need
to make sure that the merged document is authenticated and valid with respect
to it's schema. To do this we start at the roots of the hash graph and walk
down the tree. As we encounter each change we check it's signature, apply it
and check that the new document does not violate the schema. If it does violate
the schema we discard the change and all dependent changes. Finally, we have an
authenticated document which respects a given schema.

It is important to note that this merging is at this point not stored in the
repository - it can be performed in memory and may be cached. The result is
that the user sees a single merged view of the object based on the contents
of the remotes they have replicated. That is, there is no additional
merge-then-commit step.

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

Therefore we add two parameters to a radicle URL:

- `collab_object_id`: specifies that a server should only consider references to the given object ID
- `collab_type_name`: specifies that a server should only consider references to the given type name

These parameters are emitted by the radicle p2p transport as headers in a 
similar fashion to the `n=` parameter for the nonce or the `ls=true`
parameter for selecting the `upload-pack-ls` service. The Radicle git server
can then use these parameters to filter the refs it operates on.


### Updating objects

To make a change to an object we load the existing messages for an object. The 
application developer provides us with the binary representation of the change
to that object. We apply the change and ensure that the new object state still
matches the object schema. At this point the state of the object may depend on
many contributions from the tracking graph - not just the ones in our own view
of the project. We now create a commit with our new change in it, referencing
all the commits containing the direct dependencies of the change as parents.


### Schema extensions

To allow for structural validation of schemas we support the [Data
Access](https://gregsdennis.github.io/json-everything/usage/vocabs-data.html)
vocabulary of JSON Schema. This allows a schema to reference other parts of a
document via a [relative JSON pointer](https://tools.ietf.org/id/draft-handrews-relative-json-pointer-00.html)
when expressing constraints.

#### `rad_signed_by`

Many collaborative data structures will need to make statements about who is
allowed to change what parts of a structure. To achieve this we extend the 
json schema language with some custom metadata, the `rad_signed_by` property. This
property can be placed on any `object` schema. It's value is an
object with two keys, an array of properties which must be signed, and array of 
radicle URNs who's signature must be present.

This property implies a required `signatures` property with the following schema:

```json
{
    "type": "array",
    "items": {
        "type": "object",
        "properties": {
            "key": {
                "type": "string",
                "$comment": "A multibase base32-z encoding of the public key"
            },
            "signature": {
                "type": "string",
                "$comment": "A multibase base32-z encoding the signature"
            }
        }
    }
}
```

Any schema which has this property will result in some additional validation.
Librad will encode the relevant keys of the target object using CBOR.
and then check that a signature over them is valid with respect to the given
keys. Note that the encoding will go directly from automerge types to CBOR,
which allows for signatures over any type in the automerge data model, in 
particular including floating point numbers and byte types.


#### `frozen`

Some attributes should never be changed, for example the ID of an issue, or a
nonce on a comment. Any schema can add the metadata key, `frozen: true` to
indicate that after it's initial creation any change which modifies it is
invalid.

#### `grow_only`

This key stipulates that elements can never be removed from a sequence or 
object. Any schema which has this key on it will result in schema validation
for a change which removes items from the instance in question.

#### `automerge_type`

This can take the value `"text"` if  placed on `string` properties to indicate
that they should be represented in an `Automerge.Text` data type.


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

This new api will live in a new top level module at
`librad::collaborative_objects`. An initial sketch looks like this:

```rust
struct CollaborativeObjectStore {
    storage: git::storage::Pool,
    signer: signer::Signer,
}

enum History {
    Automerge(Vec<Vec<u8>>)
}

struct ObjectId(String);
struct TypeName(String);
struct Schema(..);

struct CollaborativeObject {
    typename: TypeName,
    schema: Schema,
    id: ObjectId,
    author: Person, 
    json: serde_json::Value,
    history: History, 
}

struct NewObjectSpec {
    typename: TypeName,
    history: History,
    schema_json: serde_json::Value,
}

impl CollaborativeObjectStore {
    fn retrieve_objects(&self, typename: String) -> Result<_, Vec<CollaborativeObject>>
    fn retrieve_object(&self, typename: String, id: ObjectId) -> Result<_, CollaborativeObject>
    fn create_object(&self, spec: NewObjectSpec) -> Result<_, CollaborativeObject>
    fn update_object(&self, id: String, changes: History) -> Result<_, CollaborativeObject>
}
```


## Blessed Data Types

This project metadata mechanism is extremely broad, which has a lot of upsides
but it runs the risk of running into XMPP style extension hell, where every
peer is running a different set of extensions. It might be worthwhile to bundle
a few core extensions with librad - issues for example.

## Alternative Approaches

### Domain Specific CRDTs

Instead of using a single CRDT implementation (Automerge) for every data type
we could have a CRDT per data type. Defining a CRDT consists of either 
defining a commutative merge operation for a data structure, or a set of 
operations with a commutative application operation (these are in some sense
interchangable definitions).

As an example, we might define the issue CRDT using a set of events like this: 

```rust
enum Event { Create(id, title, description, author, signature),
    Modify(new_title, new_description, new_signature),
    AddComment(id, text, author, parent_id, signature),
    ModifyComment(comment_id, text, new_signature),
    RemoveComment(comment_id, nonce, signature),
}
```

A state

```rust
struct Issue {
    title: String,
    author: Author,
    signature: Signature,
    comments: CommentTree
}

enum CommentTree {
    Node(NodeId, Vec<CommentTree>),
    Leaf(NodeId, Comment)
}

struct Comment {
    text: String,
    author: Author,
    signature: Signature,
}
```

and an apply function:


```rust
impl Issue {
    fn apply(&mut self, op: Event) {
        ...
    }
}
```

This initially seems appealing as the event log matches a little more closely
with the network model than shipping around automerge states. It's more
intuitive to think of events as happening concurrently in different places
and merging them. Furthermore, this approach makes schema validation easier,
we just have to check that the events are well formed - the final state is 
guaranteed to be valid by the merge function.

This architecture would mean that the responsibilities of the
radicle protocol would be to provide a causal broadcast system - a guarantee
that events will arrive in causal order, i.e after their dependencies, at each
node. 

There are difficulties with this approach though: 
- How do we represent the merge operation? The only general mechanism here
  would be a programming language, either source code or WASM blobs. This could
  be achieved but we would need to do some engineering to sandbox such
  programs. 
- Writing a correct CRDT merge operation is tricky and the consequences of
  getting it wrong are permanently corrupted data for the whole network. There
  are other formulations of CRDTs which make different tradeoffs in the design
  of the merge operation, but everything I am aware of requires a reasonable
  amount of domain expertise. 
- Handling upgrades seems complicated, every CRDT implementation would need to
  be able to tolerate unknown events or states.
- Even if the merge operation is correct, naive CRDT implementations can easily
  require large amounts of storage and network resources.

To me this approach seems to fail at satisfying the interoperability design
goal. We would require application developers to know how to develop a CRDT and
we would not be able to make many guarantees to users about how CRDTs will 
perform both in terms of the performance of the merge function and in terms of
disk and network usage. Additionally we open ourselves up to the security
problems of sandboxing arbitrary programs.

### JSON Patch instead of Automerge

Automerge is a reasonably esoteric technology, why are we exposing it in our
API? The reason we receive changes as a set of automerge changes - bytes 
created by the automerge library by the application developer - is that we
cannot just allow people to directly update the state of the CRDT. Doing so 
would lose crucial information which allows for good merge behaviour. For
example, when modifying a list we want to track exactly where in the list
modifications happen - just diffing states doesn't allow us to capture things
like "insert after element 3, then delete element 3, then insert after element
two", we would just end up with "delete element 3 and insert two new
elements", which would behave differently in the presence of concurrent inserts
after element 3.

However, we could use a different change format, JSON patch is reasonably well
known and straightforward to use. The problem is that it doesn't have a way of
expressing changes _within_ a string. If you want to change some text you just
change the whole property. There are [attempts to extend it](https://github.com/epoberezkin/extended-json-patch)
but these are not well known or maintained. This is a problem because one of
the most useful things about automerge is it's ability to merge text changes
in an intuitive manner.