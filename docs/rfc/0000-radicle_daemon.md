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

* Identity
  * read
  * write
* Replication
* Profile

We will sketch these capabilities here, but this may not reflect the final
definition that will be found in the implementation.

### Identity Capabilities

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
```

### Git Implementation

## HTTP

### Endpoints

## CLI

## Reactor

## Git Server

## Conclusion


