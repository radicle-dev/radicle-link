# Radicle Link E2E Testing

Tooling and utilities for E2E testing of `radicle-link` networks.

## Lo-fi Local Devnet

Prequisites:

    * [overmind](https://github.com/DarthSim/overmind)
    * [podman](https://podman.io)

Run [`./localnet`](./localnet) to compile a simple peer against `HEAD`, and run
it. Once it started up, you can watch some (rudimentary) metrics via the 'theus
web interface at [http://localhost:9090](http://localhost:9090).

You can adjust the number of peers by setting the `NUM_PEERS` environment
variable.

_Note that this is currently simply a debugging aid. More sophisticated
orchestration may be added in the future (contributions welcome!)_
