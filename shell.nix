{ sources ? import ./nix/sources.nix
, pkgs ? import sources.nixpkgs {
    overlays = [ (import sources.rust-overlay) ];
  }
}:
let
  # TODO: remove once cargo-nextest is available in nixpkgs stable
  cargo-nextest = (pkgs.callPackage ./nix/cargo-nextest/default.nix { });
  stable = pkgs.rust-bin.stable.latest.default;
  rust = stable.override {
    extensions = [ "rust-src" "rust-analysis" ];
  };
in
  with pkgs;
  mkShell {
    name = "development";
    buildInputs = [
        cargo-deny
        cargo-expand
        cargo-nextest
        cargo-watch
        pkgs.rust-bin.nightly."2021-12-02".rustfmt
        cmake
        openssl
        pkgconfig
        ripgrep
        rust
    ];
  }
