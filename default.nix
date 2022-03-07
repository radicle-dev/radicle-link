{ sources ? import ./nix/sources.nix
, pkgs ? import sources.nixpkgs {
    overlays = [ (import sources.rust-overlay) ];
  }
}:
let
  # TODO: remove once cargo-nextest is available in nixpkgs stable
  cargo-nextest = (pkgs.callPackage ./nix/cargo-nextest/default.nix { });
  rust = pkgs.rust-bin.stable.latest.default;
in
  with pkgs;
  mkShell {
    name = "build";
    buildInputs = [
        # cargo tooling
        cargo-deny
        cargo-nextest
        cargo-watch
        pkgs.rust-bin.nightly."2021-12-02".rustfmt

        # hard dependencies
        cmake
        openssl
        pkgconfig
        rust
    ];
  }
