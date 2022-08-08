//! Integration test to exercise the programs in `bins`.

use pty::fork::*;
use std::{
    env,
    io::{Read, Write},
    process::Command,
};

/// This test is inspired by https://github.com/alexjg/linkd-playground
#[test]
fn happy_path_to_push_changes() {
    let peer1_home = "/tmp/link-local-1";
    let peer2_home = "/tmp/link-local-2";
    let seed_home = "/tmp/seed-home";
    let passphrase = b"play\n";

    println!("create lnk homes for two peers and one seed");
    if !run_lnk(LnkCmd::ProfileCreate, peer1_home, passphrase) {
        return;
    }
    if !run_lnk(LnkCmd::ProfileCreate, peer2_home, passphrase) {
        return;
    }
    if !run_lnk(LnkCmd::ProfileCreate, seed_home, passphrase) {
        return;
    }

    println!("add ssh keys");
    if !run_lnk(LnkCmd::ProfileSshAdd, peer1_home, passphrase) {
        return;
    }
}

enum LnkCmd {
    ProfileCreate,
    ProfileSshAdd,
}

/// Runs a `cmd` for `lnk_home`. Rebuilds `lnk` if necessary.
/// Returns true if this is the parent (i.e. test) process,
/// returns false if this is the child (i.e. lnk) process.
fn run_lnk(cmd: LnkCmd, lnk_home: &str, passphrase: &[u8]) -> bool {
    let fork = Fork::from_ptmx().unwrap();
    if let Some(mut parent) = fork.is_parent().ok() {
        parent.write_all(passphrase).unwrap();
        println!("wrote passphase for {}", lnk_home);

        let mut output = String::new();
        parent.read_to_string(&mut output).unwrap();
        println!("{}: {}", lnk_home, output.trim());

        true
    } else {
        // Child process is to run `lnk`.
        let package_dir = env!("CARGO_MANIFEST_DIR");
        let manifest_path = format!("{}/Cargo.toml", package_dir.strip_suffix("/tests").unwrap());
        // println!("manifest_path: {}", &manifest_path);

        // cargo run \
        // --manifest-path $LINK_CHECKOUT/bins/Cargo.toml \
        // -p lnk -- "$@"
        let mut lnk_cmd = Command::new("cargo");
        lnk_cmd
            .env("LNK_HOME", lnk_home)
            .arg("run")
            .arg("--manifest-path")
            .arg(manifest_path)
            .arg("-p")
            .arg("lnk");
        let full_cmd = match cmd {
            LnkCmd::ProfileCreate => lnk_cmd.arg("--").arg("profile").arg("create"),
            LnkCmd::ProfileSshAdd => lnk_cmd.arg("--").arg("profile").arg("ssh").arg("add"),
        };
        full_cmd.status().expect("lnk profile create failed:");

        false
    }
}
