## Objectives

* [x] 2021
* [ ] 2022

  Self-host `radicle-link` on `radicle-link`.

  * Exchange code
  * Find projects
  * Manage patches
  * Task tracking

## Networking

* [ ] Provider Cache

  > Remember `Have`s in a _k_-bucket structure acc. to local subscription and/or
  > hash value. Can answer `Want` from cache; can locate providers based on
  > distance metric. Unclear how to perform liveness check when protocol is
  > stateful (TLS handshake).

* [ ] NAT Traversal

  * [ ] Hole Punching

    > Requires coordination when NAT is endpoint-dependent. Coordinator nodes
    > could be designated though a DHT, but latency and churn are problematic.
    > Maybe Chord?

* [ ] Gossip Membership Groups

  > Prefer peers with similar interests. Pubsub protocols (eg. gossipsub,
  > episub) are suitable for single-root routing trees, which is not quite what
  > we're after; in pubsub terms, we would have the URN as the topic, while
  > every tracking peer forms a sub-topic (to which others interested in URN may
  > or may not be subscribed). Maybe SCRIBE (Pastry) anycast? Look at Matrix
  > "pinecone"?

## Replication

* [ ] Stabilization

  * [ ] Prune refs
  * [ ] Avoid `ls-refs` in fetch step

    > Would fail the entire fetch if tips mentioned in `signed_refs` are missing

  * [X] Transactional tracking updates
  * [ ] Honour tracking configuration
  * [ ] Scheduling of subsequent pulls

    > Relevant only for newly discovered identities for which a tracking
    > relationship already exists

  * [ ] Single-pass validation

* [ ] Grafting

  > Mutual synchronisation

  * [x] Interrogation
  * [ ] [RFC 701](https://lists.sr.ht/~radicle-link/dev/%3C20220106191802.13292-1-kim%40eagain.st%3E)
    * [ ] Finalise
    * [ ] Implement

## Identities

* [ ] Simplify verification by requiring history to be linear
* [ ] Simplify initialising a multisig identity

  > Verification may allow the initial revision to have only one signature,
  > regardless of the number of delegations.

* [ ] Quorum overrides for personal ids

  > Cross-signing from multiple devices is inconvenient for personal ids.
  > Macaroons could be issued which allow to confirm a change using only one
  > second factor (HSM, password manager, ..). Or maybe just make the quorum
  > threshold configurable.

* [ ] Extension points for cobs (TBD)

## Collaborative Objects

* [ ] Tasks

  > Turn this document into a CRDT.

* [ ] Patches

  > Track code and comment on it.

  * [ ] Leaderless verifiable multi maintainer (fearless also)

* [ ] To ACL or to not ACL? Moderate or restrict?
* [ ] Discovery of cobs from untracked peers

  > Indexing cob (cob-of-cobs)? Interrogation API?

* [ ] CLI
* [ ] Editor Plugin?

## Tools & Infrastructure

* [ ] AppArch MVP

  * [ ] Packaging
  * [ ] systemd / launchd configs
  * [ ] linkd

    * [ ] "Announce" RPC
    * [ ] RPC triggering RFC701-style sync (single or multiple peers)

      > May provide timer file, which people may or may not activate

    * [ ] RPC triggering clone of URN
    * [ ] macOS socket activation
    * [ ] Configure / override bootstrap peers

      > Do we need to distinguish between bootstrap and custodial peers?

  * [ ] git daemon

    * [ ] Clone-through of URN (experimental)

* [ ] Instrumentation (metrics)
* [ ] git maintenance

  - git config
  - delta islands?
  - systemd/launchd timers

## Miscellanea

* [ ] Integrate new tracking in `rad-identities`
* [ ] Extract networking into crate
* [ ] Extract storage into crate
* [ ] Implement storage in terms of `link-git`
