# RFC: Identity Resolution

* Author: @FintanH, @xla
* Date: 2021-05-14
* Status: draft

## Motivation

Originally, the `radicle-link` served as the home of the core protocol, along
with some helper crates. The `radicle-upstream` project consisted of a `proxy`
and the its `ui` code. The `proxy` served as a HTTP layer so the `ui` could
interact with the `radicle-link` code.

The evolution continued and the `proxy` code was split into two sub-crates:
`api` and `coco`. The `coco` crate directly used `radicle-link` and built
smaller protocols to serve `radicle-upstream`'s needs, e.g. the waiting room,
fetch-syncing, announcement loop, etc. The `api` crate consisted of the HTTP
endpoints as well as some domain types, again serving `radicle-upstream`'s
needs.

The distance between the `coco` crate and its dependency `librad` caused a lot
of churn when major changes were made in the latter, causing weeks/months of
integration work to catch up to the latest and greatest. As well as this, it
made it harder to gauge whether code being added to `coco` could have been
useful to be in `librad` instead.

This made us make the first move to migrating the `coco` crate over to
`radicle-link` under the name `daemon` – see
[#576](https://github.com/radicle-dev/radicle-link/pull/576).

This RFC wants to tackle the next phase of this plan and give a concrete plan
for implementing a general purpose `daemon` that can serve `radicle-upstream`
and any other applications that would benefit from a high-level API on top of
`librad` et al.

## Overview

To learn from our past mistakes, we would like to move forward in a way that
identifies core components and design them in such a way that allows us to
compose them easily, allowing upstream consumers to mix-and-match them in any
way they desire. This desire leads us to the following architectural layout. A
core API that defines the capabilities for working with `radicle-link` data. Two
consumer-level packages HTTP and CLI for building interesting applications and
workflows from the core. A reactor to our core that defines daemon-level
protocols and is ultimately the running process for the `daemon`. And finally, a
git server that is specific to the git implementation of `radicle-link` allowing
us to use the `git` CLI for Radicle purposes.

## Core

The goal for the `core` is to make it as reusable as possible, all the while
making sure that it remains stable as an API, only making additions to it rather
than changing its surface area.

To do this we propose that the core API consist of capabilities that are defined
as traits. A capability would be defined for a single resource, however, it
could be split into sub-capabilities, for example, if there is a set of
read methods and a set of write methods.

Remark: a capability in this case is set of methods that define the ways one can
interact with the data the capability is defined for. For example, a capability
for a directory might be `touch`, `ls`, `mkdir`. It is defined as trait and can
be given multiple implementations.

The following capabilities our already in scope at the time of writing this RFC,
but naturally, more will be added as the project evolves.

* Gossip
* Identity
  * read
  * write
* Profile
* Replication
* Tracking

We will sketch these capabilities here, but this may not reflect the final
definition that will be found in the implementation.

### Identity

The identity capabilities define the create, read, and update methods
for interacting with Radicle's family of `Identity` types. See
[spec/identities][id] for more details.

```rust
pub trait read::Identity<I> {
	type Error;
	
	fn get<R>(&self, urn: Urn<R>) -> Result<Option<I>, Self::Error>;
	fn list<R>(&self) -> Result<impl Iterator<Item = Result<I, Self::Error>, Self::Error>;
}

pub trait write::Identity<I> {
	type Error;
	
	fn create<P, D>(&self, payload: P, delegations: D) -> Result<I, Self::Error>;
	fn update<P, D>(&self, urn: Urn<R>, payload: P, delegations: D) -> Result<I, Self::Error>;
	fn merge<R>(&self, urn: Urn<R>, from: PeerId) -> Result<I, Self::Error>;
}

pub trait read::Rad<I> {
	type Error;
	type Person;
	type Signatures;
	
	/// rad/self
	fn whoami(&self, urn: Urn<R>, peer: Option<PeerId>) -> Result<Option<Self::Person>, Self::Error>;
	
	/// rad/signed_refs
	fn signatures(&self, urn: Urn<R>, peer: Option<PeerId>) -> Result<Option<Self::Signatures>, Self::Error>;
	
	/// rad/ids/*
	fn delegates(&self, urn: Urn<R>, peer: Option<PeerId>) -> Result<impl Iterator<Item = Result<Self::Person, Self::Error>>, Self::Error>;
	
	/// rad/ids/<id>
	fn delegate(&self, urn: Urn<R>, peer: Option<PeerId>) -> Result<Option<Self::Person>, Self::Error>;
}
```

The `I` parameters signify that the traits are open to many
identities, and for now there would be specific implementations for
`Person` and `Project`. We leave any domain specific types open using
associated types on the trait, e.g. `Rad::Person = Person` will be
associated type for a `Project`'s `rad/self`.

### Tracking

As well as the family of `Identity` capabilities, we will also want
methods for the tracking graph of an identity, sketched below:

```rust
pub trait Tracking {
	type Error;
	type Tracked;
	
	fn track<R>(&self, urn: Urn<R>) -> Result<bool, Self::Error>;
	fn untrack<R>(&self, urn: Urn<R>) -> Result<bool, Self::Error>;
	fn tracked<R>(&self, urn: Urn<R>) -> Result<Tracked, Self::Error>;
}
```

### Profile

The profile capability defines how a person can create a profile, read
the current profile, and switch between existing profiles. 

```rust
pub trait Profile {
	type Error;
	type LocalIdentity;
	
	fn create(&self, whomai: Self::LocalIdentity) -> Result<(), Self::Error>;
	fn update(&self, whoami: Self::LocalIdentity) -> Result<(), Self::Error>;
	fn current(&self) -> Result<Option<Self::LocalIdentity>, Self::Error>;
	fn switch(&self, other: Self) -> Result<Self, Self::Error>;
}
```

There is an open question of how `Profile` will interact with a
backing implementation that needs to be aware about restarting a
loop. For example, if the backing implementation is `net::Peer`, then
it will need to restart the `Peer` so it can re-initialise with the
new `Storage`.

### Replication

TODO

```rust
pub trait Replication {
	type Error;
	type Result;
	
	fn replicate(&self, /* TODO */) -> Result<Self::Result, Self::Error>;
}
```

### Gossip

TODO

```rust
pub trait Gossip {
	fn query(&self, want: Payload) -> Result<(), Payload>;
	fn announce(&self, have: Payload) -> Result<(), Payload>;
}
```

### Git Implementation

The first backing implementation of these capabilities will be the
`radicle-link`'s git implementation. More precisely, they will be
implemented for the `Storage` type. It should be
possible to then chain implementations, e.g. `Peer` can use its
underlying `Storage` (FIXME: but what about async on
`using_storage`). The exception is that `Storage` does not have the
capability to `Gossip`, instead this will be directly from the `Peer`.

FIXME: Maybe there's warrant to split capabilities even further
between storage concepts and protocol concepts.

## HTTP

The HTTP component will be defined in terms of the `core`
API. The goal is to define a set of endpoints that will remain stable
but, again, will be maleable for use with different backing
implementations. This will inherently come from the capability model
discussed in [Core][#core].

### HTTP Library

TODO: do we just want to use warp? Or survey around?

### Endpoints

Here we will list the endpoints that will be included as part of the
implementation of this RFC, but will not be an exhaustive list of
endpoints the `daemon` will serve. For that we should consider looking
for [OpenAPI][open] generation.

#### Projects

* GET /project/<urn>

TODO

## CLI

The CLI component will be defined in terms of the `core` API, similar
in vein to the [HTTP component][#http]. 

### Client CLI

What we will want to consider here is to make the HTTP component an
implementor of the capabilities. By doing this the CLI can simply
become a client of the HTTP endpoints and perform requests, parsing
responses for a better `stdout` experience.

This would require that a service is always running for the CLI to be
used. One benefit of this is that if there is a running service this
means the `radicle-link` protocol is running and capabilities that
require gossip can execute.

Note that this shouldn't always need to be the case. For example,
retrieving a project only requires the computation to read from disk
and doesn't need any of the protocol to be running.

### Non-HTTP CLI

As mentioned in the previous section, a HTTP service is not always
required by a lot of operations. A CLI that simply needs to access
storage on disk could be implemented to make things easier and avoid
spinning up a service.

However, for this RFC, we leave this out of scope to focus on getting
on full functionality up-and-running first (Contributors welcome =]).

## Reactor

We have discussed capabilities and we have ensured that we delay the
implementations of these capabilities to the last moment. And so, at
the top of our stack is the `reactor` to our `core`. 

The `reactor` component will implement the `core` API by choosing the
backing implementation, git storage for the first
implementation. Since it will be implementing these capabilities, it
can be plugged into the HTTP and CLI components to acquire a running
service.

The `reactor` component will also house sub-protocols that may not fit
in `librad`, but are deemed useful for the `daemon`. Note that these
sub-protocols could be phased out if `librad` implements a protocol
that would better suit the purpose of the sub-protocol. Currently,
there are some sub-protocols that exist in the current iteration of
the `daemon`, and their use can be evaluated as part of this RFC.

* Announcements
* Synchronisation via fetch
* The waiting room, i.e. querying and waiting for a project

TODO: I'd rather discuss these via the PR or meetings and summarise
points here.

### Announcements

### Synchronisation

### Waiting Room

## Git Server

The final portion of this RFC is to consider the implementation of a
git server as part of the `daemon`. 

TODO: Actually I need to look into some git server details before I
write this =]

## Conclusion

TODO: I'll conclude when there's more conclusiveness =]

[id]: ../spec/002-identities/index.md
[open]: https://swagger.io/specification/
