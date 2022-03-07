{ sources ? import ../sources.nix
, pkgs ? import sources.nixpkgs
}:
with pkgs;
rustPlatform.buildRustPackage rec {
  pname = "cargo-nextest";
  version = "0.9.9";

  src = fetchFromGitHub {
    owner = "nextest-rs";
    repo = "nextest";
    rev = "cargo-nextest-${version}";
    sha256 = "sha256-1s1N126S51kg7aOgAb8oMts1zJcO6QRn1fwbQf6ZaJ8=";
  };

  cargoSha256 = "sha256-JxZyl5Hti3Hh33e7H/pXhM6WkU0kDDml0naBPYzvNy4=";

  nativeBuildInputs = [
    pkg-config
  ];

  buildInputs = [
    openssl
    libiconv
  ] ++ lib.optionals stdenv.isDarwin [
    Security
  ];

  doCheck = false;
}
