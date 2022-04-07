{ sources ? import ./nix/sources.nix
, pkgs ? import sources.nixpkgs {
    overlays = [ (import sources.rust-overlay) ];
  }
}:
let
  stable = pkgs.rust-bin.stable.latest.default;
  rust-overlay = stable.override {
    extensions = [ "rust-src" "rust-analysis" ];
  };
  devault = (pkgs.callPackage ./default.nix {});
in
  with pkgs;
  mkShell {
    name = "development";
    buildInputs = devault.buildInputs ++ [
        ripgrep
    ];
  }
