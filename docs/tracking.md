
# Tracking

In this section we're going to explore and explain the current[0] implementation of tracking in
`radicle-link`. Tracking is the backbone of collaboration as it drives forward the exchange of
projects and their artifacts. So we're first going to build up to say what it means "to track a peer".

A "peer" is a single device in the network. It has its own view of the network and tracks projects,
users, etc. in its own monorepo[1]. When one peer tracks another, it is tracking a particular
artifact in the network, but let's simplify our explanations here and talk about projects in
particular. So, when a peer tracks another it sets the intention to follow the other peer's progress
on this project. The metadata that describes the project, what branches the other peer is creating,
what commits they're making to which branches, and of course, the peer may want to see these changes
to merge them into their own branches.

## How Do We Track?

### Cloning

Tracking is best understood through the lens of the monorepo and how it changes while our peer
interacts with the network. Better yet, we are going to do away with generic names and have a fun
example (for my definition of fun). Our friend Kermit the Frog wants to start a Muppets Show project
and get all his friends involved. So he starts off by creating a project and his monorepo will look
like the following:

```
refs
`-- namespaces
    |-- $THE_MUPPET_SHOW
    |   `-- refs
    |       `-- heads # <-- code branches owned by $KERMIT go here
    |           `-- muppet-mania
    |       `-- rad
    |           |-- id # <-- points to the identity document history
    |           |-- signed_refs # <-- signed refs of the peer
    |           |-- self # <-- points to the identity of $KERMIT_IDENTITY
    |           `-- ids
    |               `-- $KERMIT_IDENTITY
    `-- $KERMIT_IDENTITY
        `-- refs
            |-- heads
            |-- rad
                |-- id
                |-- signed_refs
                |-- self
                `-- ids
                    `-- $KERMIT_IDENTITY
```

In the above `$THE_MUPPET_SHOW` will be a hash value that acts as a stable identifier for the
project in the network. `$KERMIT` refers to the unique peer identifier for Kermit's device. And
`$KERMIT_IDENTITY` refers to Kermit's identity document that has metadata such as his handle and
public keys[2].

Kermit, ever the romantic, decides to get his beloved Miss Piggy involved in the Muppet Show and
tells her the URN that identifies the project. She asks the network for the URN and Kermit's peer
responds with the project. So what would Miss Piggy's monorepo look like?

```
refs
`-- namespaces
    |-- $THE_MUPPET_SHOW
    |   `-- refs
    |       |-- heads # <-- code branches owned by $MISS_PIGGY go here
    |       |   `-- muppet-mania
    |       |-- rad
    |       |   |-- id
    |       |   |-- signed_refs
    |       |   |-- self # <-- points to the identity of $MISS_PIGGY
    |       |   `-- ids
    |       |       `-- $KERMIT
    |       `-- remotes
    |           `-- $KERMIT
    |               |-- heads
    |               |   `-- muppet-mania
    |               `-- rad
    |                   |-- id # <-- points to the identity of $KERMIT_IDENTITY
    |                   |-- signed_refs
    |                   |-- self
    |                   `-- ids
    |                       `-- $KERMIT_IDENTITY
    |-- $KERMIT_IDENTITY
    |   `-- refs
    |       |-- heads
    |       `-- rad
    |           |-- id
    |           |-- signed_refs
    |           |-- self
    |           `-- ids
    |               `-- $KERMIT_IDENTITY
    `-- $MISS_PIGGY_IDENTITY
        `-- refs
            |-- heads
            `-- rad
                |-- id
                |-- signed_refs
                |-- self
                `-- ids
                    `-- $MISS_PIGGY_IDENTITY
```

Again, `$MISS_PIGGY` and `$MISS_PIGGY_IDENTITY` are variables for Miss Piggy's peer identifier for
the device she's using and the her identity document.

As well as this entry of `remotes/$KERMIT`, Miss Piggy's monorepo will also gain an entry in its
`config` file:

```
[remote "$THE_MUPPET_SHOW/$KERMIT"]
        url = rad-p2p://$MISS_PIGGY@$KERMIT/$THE_MUPPET_SHOW.git?
        fetch = +refs/heads/*:refs/remotes/$KERMIT/$THE_MUPPET_SHOW/*
```

The fetch refspec[5] here will seem off to you when comparing it to the above layout of a single
monorepo, and you would be right. In this case Link only cares about the `url` and manages the fetch
refspecs under the hood. The URL allows use to manage who we are tracking and for what URN.

What we are learning from this exposition is that we add a peer as someone we track when we clone[3]
their project. Miss Piggy is adding Kermit as a `remote`, and so she is tracking his changes. If
Kermit announces changes and Miss Piggy's peer hears about these changes -- whether through Kermit's
device or some other peer -- she will apply them to update her view of Kermit's remote.

### Directly Track

The other way a peer can track is by explicitly knowing a peer's identifier and telling their
monorepo that they are interested in this peer. For example, Miss Piggy also heard that Rizzo the
Rat is also taking part in the project and knows his peer identifier, $RIZZO. When she uses the
`track`[4] function it creates a new entry in the monorepo config:

```
[remote "$THE_MUPPET_SHOW/$RIZZO"]
        url = rad-p2p://$MISS_PIGGY@$RIZZO/$THE_MUPPET_SHOW.git?
        fetch = +refs/heads/*:refs/remotes/$RIZZO/$THE_MUPPET_SHOW/*
```

The next time she hears from the network about changes from Rizzo she will fetch those changes and
apply them to her monorepo.

## The Tracking Exchange

In the above example some extra details were elided because we were only talking about two peers. So
when Miss Piggy cloned the repository from Kermit, it was almost obvious that he would end up in the
`remotes`. But what if we talked about more peers?

Let's say Gonzo ends up cloning from Miss Piggy. We can intuit from the above that Miss Piggy will
end up as a remote in Gonzo's monorepo view of the project. But not only that, Kermit will also be
added. Kermit's role is special since he's the maintainer of the project. In fact, when any peer
clones a project from the network all the maintainers will end up in that peer's remote list, and so
the maintainers are tracked. This makes sense since maintainers have the canonical view of the
project. They're ensuring the health and consistency of this piece of work.

The fun doesn't stop there. A portion of Miss Piggy's tracking graph will also be added to Gonzo's
graph. The idea here is that Gonzo isn't just relying on who he cloned from and the maintainers, but
also some portion of the network that is interested in the project. The natural question is, what
portion of the network that Miss Piggy is tracking is also tracked by Gonzo? The answer is 2 degress
out.

In Rust this expressed as:
```rust
pub struct Remotes(HashMap<PeerId, HashMap<PeerId, HashSet<PeerId>>>);
```

So for a single `PeerId`, let's call it `p`, we have a sub-graph that consists of more `PeerId`s.
Picking one of those and call it `q`. Then `q` has a set of `PeerId`s. That is to say Miss Piggy has
a graph of 3 degress.

This is a bit of a mouthful, so let's view this pictorially:

[INSERT REMARKABLE IMAGE HERE]

So Gonzo inherits a portion of these remotes: the keys of the outer `HashMap` and the keys of the
inner `HashMap`.

Again, we show this portion pictorially:

[INSERT REMARKABLE IMAGE HERE]

## The Role of Seeds

TODO: Short explanation on seeds and why they're useful for building up a view of the network

## The Social Overlay

TODO: Explanation on 2-degree tracking and tracking your peers manually in return.

TODO: Need to make sure these are in order or just named.
[0] As of 10th September 2020
[1] TODO: link to identity resolution
[2] TODO: link to identity spec
[3] TODO: link to clone function
[4] TODO: link to track function
[5] TODO: link to git refspec
