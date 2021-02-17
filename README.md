[![Build status](https://badge.buildkite.com/c76805e51e194fb0cdf4bf537306e3b6270cb1ebc4db48f21c.svg?branch=master)](https://buildkite.com/monadic/radicle-link)

# Radicle Link 🌱

This is the working repo for the second iteration of the [Radicle] code
collaboration protocol.

**🚨 WORK IN PROGRESS 🚨**

While `radicle-link` is currently being used in the [Upstream] application it is
under heavy development. If you came here from [Upstream], please note that it
may be pinned to an earlier commit than what you see here.

Pop into [#general on our Matrix server][matrix] for development updates.

## Build

Besides a Rust build environment (best obtained using [rustup]), you may need to
install the following packages on a Debian system:

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
[Dockerfile used for CI][docker].

To compile, run `cargo build`.

## License

Unless otherwise noted, all source code in this repository is licensed under the
[GPLv3] with Radicle Linking Exception. See the [LICENSE] file for details.

If you are considering to contribute to this project, please review the
[contributing guidelines][contributing].



[Radicle]: https://radicle.xyz
[Upstream]: https://github.com/radicle-dev/radicle-upstream
[matrix]: https://matrix.radicle.community/#/room/#general:radicle.community
[rustup]: https://rustup.rs
[docker]: ./.buildkite/docker/rust/Dockerfile
[GPLv3]: https://www.gnu.org/licenses/gpl-3.0.txt
[LICENSE]: ./LICENSE
[contributing]: ./CONTRIBUTING.md
