# syntax=docker/dockerfile:1.2
# Nb: the "latest" tag needs to be managed manually, CI doesn't update it
FROM gcr.io/opensourcecoin/radicle-link-seedling-build:latest AS build
WORKDIR /build
RUN --mount=type=bind,source=.,target=/build,rw \
    --mount=type=cache,target=/cache \
    set -eux pipefail; \
    mkdir -p /cache/target; \
    ln -s /cache/target target ; \
    cargo build --release --package radicle-link-e2e --bin ephemeral-peer; \
    mv target/release/ephemeral-peer /ephemeral-peer

FROM debian:buster-slim
RUN set -eux; \
    apt update; \
    apt install -y --no-install-recommends \
        ca-certificates \
        git \
    ; \
    apt-get autoremove; \
    rm -rf /var/lib/apt/lists/*
COPY --from=build /ephemeral-peer /usr/local/bin/ephemeral-peer
CMD ["ephemeral-peer"]
