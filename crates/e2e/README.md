# Radicle Link E2E Testing

Tooling and utilities for E2E testing of `radicle-link` networks.

## Prequisites

* [podman] or [docker]
* [docker-compose]
* Optional: [overmind]

## Usage

To quickly spin up a few peers without waiting for slow container builds, run
the `quick` script. This will start `$NUM_PEERS`, plus a bootstrap node, using
[overmind].

A more elaborate setup is provided via [docker-compose]. Run

    DOCKER_BUILDKIT=1 docker-compose -f compose.yaml up

to spin up a similar network topology. In addition, a [prometheus] dashboard
will be available at [http://localhost:9090](http://localhost:9090).

To force [docker-compose] to rebuild the container(s) after a code change, run:

    DOCKER_BUILDKIT=1 docker-compose -f compose.yaml up --build


[overmind]: https://github.com/DarthSim/overmind
[podman]: https://podman.io
[docker]: https://docs.docker.com/engine/
[docker-compose]: https://docs.docker.com/compose/
[prometheus]: https://prometheus.io
