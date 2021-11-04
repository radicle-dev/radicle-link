# rad

Welcome to the home of the `rad` executable.

This is still under development, so there are dark corners waiting for
you. Watch your step...

## Supported Operating Systems

[x] Linux
[x] MacOs
[ ] Windows

## Installation

### Cargo

You can install the `rad` executable from source by cloning the
`radicle-link` repository and using the [cargo] tool.

```bash
$ cargo install --path=bins/rad
```

Note that you will need to have `$HOME/.cargo/bin` on your `$PATH`.

## Usage

The `rad` executable is made up of subcommands, where subcommands can
be [extended][rad-extensions]. This section will guide you through two
of the built-in subcommands, but is not a complete guide on how to use
`rad`.

### Profile Creation

To start off, we will want to initialise a profile which sets up the
storage and key material.

```bash
$ rad profile create
```

This will prompt you for a passphrase. This is to encrypt your Radicle
key material stored on disk. **Note** that there is **no way to
recover a lost passphrase**.

Once a passphrase is created the result profile id and peer id are
printed to stdout:

```
profile id: e8ae552d-3285-405c-a156-b9b7af6daa49
peer id: hydj6zb77u518j1rxzf461s3oagyqksbp8tepsnc57mgrdn4aytphn
```

You can get your active profile id by using:

```bash
$ rad profile get
```

Or if you have multiple profiles, you may want to list them all:

```bash
$ rad profile list
```

Finally, if you want to set your active profile to a specific
identifier, you can use:

```
$ rad profile set --id e8ae552d-3285-405c-a156-b9b7af6daa49
```

### Person Creation

The next thing you may want to do is setup your `Person` through the
`identities` subcommand. This `Person` document holds the metadata
about you.

```bash
$ rad identities person create new --payload '{"name": "fintohaps"}'
```

Uh-oh, this resulted in an error:

```
Error: the key for
hydj6zb77u518j1rxzf461s3oagyqksbp8tepsnc57mgrdn4aytphn is not in the ssh-agent, consider adding it via `rad profile ssh add`
```

To have write access to the underlying storage, we also need access to
our key material. `rad` makes this easy by allowing you to add your
key to the `ssh-agent` through the following command:

```bash
$ rad profile ssh add
```

This will prompt you for your passphrase and add the key to the
session. Now let's try that again:

```bash
$ rad identities person create new --payload '{"name": "fintohaps"}'
{"urn":"rad:git:hnrkjszgfctz1dbtgyxsby56tskj98nzr81by","payload":{"https://radicle.xyz/link/identities/person/v1":{"name":"fintohaps"}}}
```

You may also want to create a `Person` with an extension, as other
applications that rely on `radicle-link` have special metadata they
would like to store as part of the `Person`. For example:

```bash
$ {"urn":"rad:git:hnrkx1r5bi59fxnq3sw64zmfygue3mzn9mgko","payload":{"https://radicle.xyz/link/identities/person/v1":{"name":"fintohaps"},"https://radicle.xyz/upstream/identities/person/eth/v1":{"address":"0x420"}}}
```

### Local Identity

For some operations -- such as `Project` creation -- it's necessary to
have a local identity, i.e. a `Person` created by the local peer. We
can set a local identity by doing the following:

```bash
$ rad identities local set --urn rad:git:hnrkx1r5bi59fxnq3sw64zmfygue3mzn9mgko
set default identity to `rad:git:hnrkx1r5bi59fxnq3sw64zmfygue3mzn9mgko`
```

### Project Creation

Now let's initialise a `Project` based on an existing project on your
computer. This is similar to how we created a `Person` but the initial
metadata includes an optional `description` and `default_branch`. As
well as this, since we're using an existing `git` project, we use the
`existing` option and must pass a `--path` where the project can be found:

```bash
$ rad identities project create existing --path /home/haptop/Developer --payload '{"name": "radicle-link", "default_branch": "master"}'
{"urn":"rad:git:hnrkf3ps37d5xk9huh7unhf7ryg1k76yhfk4o","payload":{"https://radicle.xyz/link/identities/project/v1":{"name":"radicle-link","description":null,"default_branch":"master"}}}
```

### Help?

There are more commands available, and all of them have help
information attached. So, if at any point you're wondering what
something does and you want to explore then passing `--help` will
print out some, hopefully, useful prose.

[cargo]: https://doc.rust-lang.org/cargo/
[rad-extensions]: https://github.com/radicle-dev/radicle-link/blob/master/docs/rfc/0698-cli-infrastructure.adoc#the-fellowship-of-the-rad
