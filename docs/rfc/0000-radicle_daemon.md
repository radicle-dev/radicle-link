# RFC: Identity Resolution

* Author: @FintanH, @xla
* Date: 2021-05-14
* Status: draft

## Motivation

Originally, the `radicle-link` served as the home of the core protocol, along with some helper crates. The `radicle-upstream` project consisted of a `proxy` and the its `ui` code. The `proxy` served as a HTTP layer so the `ui` could interact with the `radicle-link` code.

The evolution continued and the `proxy` code was split into two sub-crates: `api` and `coco`. The `coco` crate directly used `radicle-link` and built smaller protocols to serve `radicle-upstream`'s needs, e.g. the waiting room, fetch-syncing, announcement loop, etc. The `api` crate consisted of the HTTP endpoints as well as some domain types, again serving `radicle-upstream`'s needs.

The distance between the `coco` crate and its dependency `librad` caused a lot of churn when major changes were made in the latter, causing weeks/months of integration work to catch up to the latest and greatest. As well as this, it made it harder to gauge whether code being added to `coco` could have been useful to be in `librad` instead.

This made us make the first move to migrating the `coco` crate over to `radicle-link` under the name `daemon` – see [#576](https://github.com/radicle-dev/radicle-link/pull/576).

This RFC wants to tackle the next phase of this plan and give a concrete plan for implementing a general purpose `daemon` that can serve `radicle-upstream` and any other applications that would benefit from a high-level API on top of `librad` et al.

## Overview

- What this rfc will cover
  - core api
  - http
  - cli
  - git server
  - reactor
  - background process

## Core

### Git Implementation

## HTTP

## CLI

## Reactor

## Git Server

## Conclusion


