FROM debian:buster-slim

# System packages
RUN set -eux; \
    apt-get update; \
    apt-get install -y --no-install-recommends \
        ca-certificates \
        cmake \
        curl \
        file \
        gcc \
        git \
        libc6-dev \
        liblzma-dev \
        libssl-dev \
        make \
        pkg-config \
        ; \
    apt-get autoremove; \
    rm -rf /var/lib/apt/lists/*

# Rust toolchain
# Make sure this is in sync with rust-toolchain!
ENV RUST_VERSION=nightly-2021-06-17 \
    CARGO_HOME=/usr/local/cargo \
    PATH=/usr/local/cargo/bin:$PATH \
    RUSTUP_HOME=/usr/local/rustup \
    RUSTUP_VERSION=1.24.1 \
    RUSTUP_SHA512=d85a0499bf98163c42181d6720572bf2d0c1538a252b528b9812ea876a68d486e4720cc236640bda4c241d98aebbf3f83d42cf551522bf2f4837aa048c198714

RUN set -eux; \
    curl -LOf "https://static.rust-lang.org/rustup/archive/${RUSTUP_VERSION}/x86_64-unknown-linux-gnu/rustup-init"; \
    echo "${RUSTUP_SHA512} *rustup-init" | sha512sum -c -; \
    chmod +x rustup-init; \
    ./rustup-init -y --no-modify-path --profile minimal --default-toolchain $RUST_VERSION; \
    rm rustup-init; \
    chmod -R a+w $RUSTUP_HOME $CARGO_HOME; \
    rustup --version; \
    cargo --version; \
    rustc --version; \
    rustup component add clippy rustfmt; \
    cargo install cargo-deny; \
    rm -rf /usr/local/cargo/registry; \
    rm /usr/local/cargo/.package-cache;

VOLUME /cache
ENV CARGO_HOME=/cache/cargo
