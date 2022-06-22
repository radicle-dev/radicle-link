Here's how to get a simple lnk setup up and running on linux using systemd.

## Build and install the CLI tools

```
cd bins
cargo install --path lnk-gitd
cargo install --path lnk
```

## Setup your local identity

```
# create a profile
lnk profile create
# create your person identity, record the `urn` field of the resulting JSON
lnk identities person create --payload '{"name": "<your display name>"}'
lnk identities local set --urn <the urn from above>
```

## Add your seeds

Modify `$XDG_CONFIG_DIR/.radicle-link/<profile ID>/seeds`, add the following
line:

```
<your seed peer ID>@<address>:<port>
```

Note that the active profile can be found with `lnk profile get`


## Create the unit files

Copy `example-systemd-config/lnk-gitd.service` to `$XDG_CONFIG_DIR/user`. Modify
it so that the `<your lnk-gitd>` points at the location of the `lnk-gitd` you
installed above (probably `$HOME/.cargo/bin/lnk-gitd`).

Copy `example-systemd-config/lnk-gitd.socket` to `$XDG_CONFIG_DIR/user`, modify
the port in `ListenStream` to be wherever you want the git server to run.

Finally, enable the socket with `systemctl --user enable lnk-gitd.socket`


## Clone a project and mess around and add remote config

```
lnk clone --urn <some project>
cd <project name>
```

You'll need to update the project config to point `rad://` urls at `lnk-gitd`.
Put the following lines in `.git/config`

```
[url "ssh://127.0.0.1:9987/rad:git:"]
    insteadOf = "rad://"
```

Note that you can't currently put this in `~/.gitconfig` as it messes up `lnk
clone`.


## Off you go

Now you can push and pull from your monorepo via `git push rad`. Currently you
will need to add remotes for each other peer you want to work, this will be
fixed once we have updated `lnk sync` to update include files.
