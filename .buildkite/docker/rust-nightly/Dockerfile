FROM rustlang/rust:nightly-slim

# Install additional packages
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        clang \
        curl \
        file \
        libclang-dev \
        libgmp-dev \
        libssl-dev \
        llvm-dev \
        m4 \
        make \
        pkg-config \
    && apt-get autoremove \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install sccache cargo-deb \
    && rm -rf /usr/local/cargo/registry \
    && rm /usr/local/cargo/.package-cache

VOLUME /cache
ENV SCCACHE_DIR=/cache/sccache \
    RUSTC_WRAPPER=sccache \
    CARGO_HOME=/cache/cargo
