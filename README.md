[![Build status](https://badge.buildkite.com/c76805e51e194fb0cdf4bf537306e3b6270cb1ebc4db48f21c.svg?branch=master)](https://buildkite.com/monadic/radicle-link)

# Radicle Link 🌱

This is the working repo for the second iteration of the [Radicle](https://radicle.xyz/)
code collaboration protocol and stack.

**🚨 WORK IN PROGRESS 🚨**

While `radicle-link` is currently being used in the `Upstream` application it is
under heavy development.

Note that [`Upstream`](https://github.com/radicle-dev/radicle-upstream) currently tracks the `maint` branch, while development happens on `next`.

Pop into [#general on our Matrix server](https://matrix.radicle.community/#/room/#general:radicle.community)
for development updates.

## Build

Besides a Rust build environment (best obtained using [rustup](https://rustup.rs)),
you may need to install the following packages on a Debian system:

* `file`
* `gcc`
* `git`
* `libc6-dev`
* `liblzma-dev`
* `libssl-dev`
* `make` (GNU make)
* `pkg-config`
* `zlib1g-dev`

For an up-to-date specification of the build and development toolchain, see the
[Dockerfile used for CI](./.buildkite/docker/rust/Dockerfile).

To compile, run `cargo build`.

## License

Unless otherwise noted, all source code in this repository is licensed under the
[GPLv3](https://www.gnu.org/licenses/gpl-3.0.txt) with Radicle Linking Exception.
See the [LICENSE](./LICENSE) file for details.

If you are considering to contribute to this project, please review the
[contributing guidelines](./CONTRIBUTING.md).
