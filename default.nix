# copied expressions from https://nixos.wiki/wiki/Rust
# and Mozilla's nix overlay README
# https://www.scala-native.org/en/latest/user/setup.html
let
  moz_overlay = import (builtins.fetchTarball https://github.com/mozilla/nixpkgs-mozilla/archive/4521bc61c2332f41e18664812a808294c8c78580.tar.gz);
  pkgs = import <nixpkgs> { overlays = [ moz_overlay ]; };
in
  pkgs.stdenv.mkDerivation {
    name = "clang-env-with-nightly-rust";
    buildInputs = with pkgs; [
      (rustChannelOf { rustToolchain = ./rust-toolchain; }).rust
      clang
      llvmPackages.libclang
      olm
      pkgconfig
      openssl
      gmp
      m4
      libiconv
      cargo-watch
      cacert
    ];
    # why do we need to set the library path manually?
    shellHook = ''
      export LIBCLANG_PATH="${pkgs.llvmPackages.libclang}/lib";
    '';
  }
