FROM rust:1.39-slim-buster

# Install additional packages
RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        clang \
        curl \
        jq \
        libclang-dev \
        libgmp-dev \
        libssl-dev \
        llvm-dev \
        lmodern \
        m4 \
        make \
        pandoc \
        pandoc-citeproc \
        pkg-config \
        texlive \
        texlive-xetex \
    && apt-get autoremove \
    && rm -rf /var/lib/apt/lists/*

RUN cargo install sccache cargo-deb \
    && rm -rf /usr/local/cargo/registry \
    && rm /usr/local/cargo/.package-cache

# Setup defaults for caching
VOLUME /cache
ENV SCCACHE_DIR=/cache/sccache RUSTC_WRAPPER=sccache CARGO_HOME=/cache/cargo
