// Copyright © 2022 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

//! Integration test to exercise the programs in `bins`.

use pty::fork::*;
use serde_json::{json, Value};
use std::{
    env,
    fs::File,
    io::{BufRead, BufReader, Write},
    process::{Child, Command, Stdio},
    thread,
    time::{Duration, SystemTime},
};

/// This test is inspired by https://github.com/alexjg/linkd-playground
///
/// Tests a typical scenario: there are two peer nodes and one seed node.
/// The main steps are:
///   - Setup a profile for each node in tmp home directories.
///   - Setup SSH keys for each profile.
///   - Create identities.
///   - Create a local repo for peer 1.
///   - Start `linkd` for the seed, and `lnk-gitd` for peer 1.
///   - Push peer 1 repo to its monorepo and to the seed.
///   - Clone the peer 1 repo to peer 2 via seed.
///
/// This test only works in Linux at this moment.
#[test]
fn two_peers_and_a_seed() {
    let peer1_home = "/tmp/link-local-1";
    let peer2_home = "/tmp/link-local-2";
    let seed_home = "/tmp/seed-home";
    let passphrase = b"play\n";

    {
        println!("\n== create lnk homes for two peers and one seed ==\n");
        let cmd = LnkCmd::ProfileCreate;
        run_lnk!(&cmd, peer1_home, passphrase);
        run_lnk!(&cmd, peer2_home, passphrase);
        run_lnk!(&cmd, seed_home, passphrase);
    }

    {
        println!("\n== add ssh keys for each profile to the ssh-agent ==\n");
        let cmd = LnkCmd::ProfileSshAdd;
        run_lnk!(&cmd, peer1_home, passphrase);
        run_lnk!(&cmd, peer2_home, passphrase);
        run_lnk!(&cmd, seed_home, passphrase);
    }

    {
        println!("\n== Creating local link 1 identity ==\n");
        let peer1_name = "sockpuppet1".to_string();
        let cmd = LnkCmd::IdPersonCreate { name: peer1_name };
        let output = run_lnk!(&cmd, peer1_home, passphrase);
        let v: Value = serde_json::from_str(&output).unwrap();
        let urn = v["urn"].as_str().unwrap().to_string();
        let cmd = LnkCmd::IdLocalSet { urn };
        run_lnk!(&cmd, peer1_home, passphrase);
    }

    {
        println!("\n== Creating local link 2 identity ==\n");
        let peer2_name = "sockpuppet2".to_string();
        let cmd = LnkCmd::IdPersonCreate { name: peer2_name };
        let output = run_lnk!(&cmd, peer2_home, passphrase);
        let v: Value = serde_json::from_str(&output).unwrap();
        let urn = v["urn"].as_str().unwrap().to_string();
        let cmd = LnkCmd::IdLocalSet { urn };
        run_lnk!(&cmd, peer2_home, passphrase);
    }

    let peer1_proj_dir = format!("peer1_proj_{}", timestamp());
    let peer1_proj_urn = {
        println!("\n== Create a local repository for peer1 ==\n");
        let cmd = LnkCmd::IdProjectCreate {
            name: peer1_proj_dir.clone(),
        };
        let output = run_lnk!(&cmd, peer1_home, passphrase);
        let v: Value = serde_json::from_str(&output).unwrap();
        let proj_urn = v["urn"].as_str().unwrap().to_string();
        println!("our project URN: {}", &proj_urn);
        proj_urn
    };

    println!("\n== Add the seed to the local peer seed configs ==\n");
    let seed_endpoint = {
        let cmd = LnkCmd::ProfilePeer;
        let seed_peer_id = run_lnk!(&cmd, seed_home, passphrase);
        format!("{}@127.0.0.1:8799", &seed_peer_id)
    };

    {
        // Create seed file for peer1
        let cmd = LnkCmd::ProfileGet;
        let peer1_profile = run_lnk!(&cmd, peer1_home, passphrase);
        let peer1_seed = format!("{}/{}/seeds", peer1_home, peer1_profile);
        let mut peer1_seed_f = File::create(peer1_seed).unwrap();
        peer1_seed_f.write_all(seed_endpoint.as_bytes()).unwrap();
    }

    {
        // Create seed file for peer2
        let cmd = LnkCmd::ProfileGet;
        let peer2_profile = run_lnk!(&cmd, peer2_home, passphrase);
        let peer2_seed = format!("{}/{}/seeds", peer2_home, peer2_profile);
        let mut peer2_seed_f = File::create(peer2_seed).unwrap();
        peer2_seed_f.write_all(seed_endpoint.as_bytes()).unwrap();
    }

    println!("\n== Start the seed linkd ==\n");
    let manifest_path = manifest_path();
    let mut linkd = spawn_linkd(seed_home, &manifest_path);

    println!("\n== Start the peer 1 gitd ==\n");
    let cmd = LnkCmd::ProfilePeer;
    let gitd_addr = "127.0.0.1:9987";
    let peer1_peer_id = run_lnk!(&cmd, peer1_home, passphrase);
    let mut gitd = spawn_lnk_gitd(peer1_home, &manifest_path, gitd_addr);

    println!("\n== Make some changes in the repo: add and commit a test file ==\n");
    env::set_current_dir(&peer1_proj_dir).unwrap();
    let mut test_file = File::create("test").unwrap();
    test_file.write_all(b"test").unwrap();
    Command::new("git")
        .arg("add")
        .arg("test")
        .output()
        .expect("failed to do git add");
    let output = Command::new("git")
        .arg("commit")
        .arg("-m")
        .arg("test commit")
        .output()
        .expect("failed to do git commit");
    println!("git-commit: {:?}", &output);

    println!("\n== Add the linkd remote to the repo ==\n");
    let remote_url = format!("ssh://rad@{}/{}.git", gitd_addr, &peer1_proj_urn);
    Command::new("git")
        .arg("remote")
        .arg("add")
        .arg("linkd")
        .arg(remote_url)
        .output()
        .expect("failed to do git remote add");

    clean_up_known_hosts();

    if run_git_push_in_child_process() {
        // The child process is done with git push.
        return;
    }

    let peer1_last_commit = git_last_commit();

    println!("\n== Clone to peer2 ==\n");

    env::set_current_dir("..").unwrap(); // out of the peer1 proj directory.
    let peer2_proj = format!("peer2_proj_{}", timestamp());
    let cmd = LnkCmd::Clone {
        urn: peer1_proj_urn,
        peer_id: peer1_peer_id,
        path: peer2_proj.clone(),
    };
    run_lnk!(&cmd, peer2_home, passphrase);

    env::set_current_dir(peer2_proj).unwrap();
    let peer2_last_commit = git_last_commit();
    println!("\n== peer1 proj last commit: {}", &peer1_last_commit);
    println!("\n== peer2 proj last commit: {}", &peer2_last_commit);

    println!("\n== Cleanup: kill linkd (seed) and gitd (peer1) ==\n");

    linkd.kill().ok();
    gitd.kill().ok();

    assert_eq!(peer1_last_commit, peer2_last_commit);
}

enum LnkCmd {
    ProfileCreate,
    ProfileGet,
    ProfilePeer,
    ProfileSshAdd,
    IdPersonCreate {
        name: String,
    },
    IdLocalSet {
        urn: String,
    },
    IdProjectCreate {
        name: String,
    },
    Clone {
        urn: String,
        peer_id: String,
        path: String,
    },
}

/// Runs a `lnk` command of `$cmd` using `$lnk_home` as the node home.
/// Also support the passphrase input for commands that need it.
#[macro_export]
macro_rules! run_lnk {
    ( $cmd:expr, $lnk_home:ident, $passphrase:ident ) => {{
        let fork = Fork::from_ptmx().unwrap();
        if let Some(mut parent) = fork.is_parent().ok() {
            match $cmd {
                LnkCmd::ProfileCreate | LnkCmd::ProfileSshAdd => {
                    // Input the passphrase if necessary.
                    parent.write_all($passphrase).unwrap();
                },
                _ => {},
            }
            process_lnk_output($lnk_home, &mut parent, $cmd)
        } else {
            start_lnk_cmd($lnk_home, $cmd);
            return;
        }
    }};
}

fn process_lnk_output(lnk_home: &str, lnk_process: &mut Master, cmd: &LnkCmd) -> String {
    let buf_reader = BufReader::new(lnk_process);
    let mut output = String::new();
    for line in buf_reader.lines() {
        let line = line.unwrap();

        // Print the output and decode them if necessary.
        println!("{}: {}", lnk_home, line);
        match cmd {
            LnkCmd::IdPersonCreate { name: _ } => {
                if line.find("\"urn\":").is_some() {
                    output = line; // get the line with URN.
                }
            },
            LnkCmd::IdProjectCreate { name: _ } => {
                if line.find("\"urn\":").is_some() {
                    output = line; // get the line with URN.
                }
            },
            LnkCmd::ProfileGet => {
                output = line; // get the last line for profile id.
            },
            LnkCmd::ProfilePeer => {
                output = line; // get the last line for peer id.
            },
            LnkCmd::Clone {
                urn: _,
                peer_id: _,
                path: _,
            } => {
                output = line;
            },
            _ => {},
        }
    }

    output
}

fn start_lnk_cmd(lnk_home: &str, cmd: &LnkCmd) {
    let manifest_path = manifest_path();

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
        .arg("lnk")
        .arg("--");
    let full_cmd = match cmd {
        LnkCmd::ProfileCreate => lnk_cmd.arg("profile").arg("create"),
        LnkCmd::ProfileGet => lnk_cmd.arg("profile").arg("get"),
        LnkCmd::ProfilePeer => lnk_cmd.arg("profile").arg("peer"),
        LnkCmd::ProfileSshAdd => lnk_cmd.arg("profile").arg("ssh").arg("add"),
        LnkCmd::IdPersonCreate { name } => {
            let payload = json!({ "name": name });
            lnk_cmd
                .arg("identities")
                .arg("person")
                .arg("create")
                .arg("new")
                .arg("--payload")
                .arg(payload.to_string())
        },
        LnkCmd::IdLocalSet { urn } => lnk_cmd
            .arg("identities")
            .arg("local")
            .arg("set")
            .arg("--urn")
            .arg(urn),
        LnkCmd::IdProjectCreate { name } => {
            let payload = json!({"name": name, "default_branch": "master"});
            let project_path = format!("./{}", name);
            lnk_cmd
                .arg("identities")
                .arg("project")
                .arg("create")
                .arg("new")
                .arg("--path")
                .arg(project_path)
                .arg("--payload")
                .arg(payload.to_string())
        },
        LnkCmd::Clone { urn, peer_id, path } => lnk_cmd
            .arg("clone")
            .arg("--urn")
            .arg(urn)
            .arg("--path")
            .arg(path)
            .arg("--peer")
            .arg(peer_id),
    };
    full_cmd.status().expect("lnk cmd failed:");
}

fn spawn_linkd(lnk_home: &str, manifest_path: &str) -> Child {
    let log_name = format!("linkd_{}.log", &timestamp());
    let log_file = File::create(&log_name).unwrap();
    let target_dir = bins_target_dir();
    let exec_path = format!("{}/debug/linkd", &target_dir);

    Command::new("cargo")
        .arg("build")
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("-p")
        .arg("linkd")
        .output()
        .expect("cargo build linkd failed");

    let child = Command::new(&exec_path)
        .env("RUST_BACKTRACE", "1")
        .arg("--lnk-home")
        .arg(lnk_home)
        .arg("--track")
        .arg("everything")
        .arg("--protocol-listen")
        .arg("127.0.0.1:8799")
        .stdout(Stdio::from(log_file))
        .spawn()
        .expect("linkd failed to start");
    println!("linkd stdout redirected to {}", &log_name);
    thread::sleep(Duration::from_secs(1));
    child
}

fn spawn_lnk_gitd(lnk_home: &str, manifest_path: &str, addr: &str) -> Child {
    let target_dir = bins_target_dir();
    let exec_path = format!("{}/debug/lnk-gitd", &target_dir);

    Command::new("cargo")
        .arg("build")
        .arg("--target-dir")
        .arg(&target_dir)
        .arg("--manifest-path")
        .arg(manifest_path)
        .arg("-p")
        .arg("lnk-gitd")
        .stdout(Stdio::inherit())
        .output()
        .expect("cargo build lnk-gitd failed");

    let child = Command::new(&exec_path)
        .arg(lnk_home)
        .arg("--push-seeds")
        .arg("--fetch-seeds")
        .arg("-a")
        .arg(addr)
        .spawn()
        .expect("lnk-gitd failed to start");
    println!("started lnk-gitd");
    child
}

/// Returns true if runs in the forked child process for git push.
/// returns false if runs in the parent process.
fn run_git_push_in_child_process() -> bool {
    let fork = Fork::from_ptmx().unwrap();
    if let Some(mut parent) = fork.is_parent().ok() {
        let yes = b"yes\n";
        let buf_reader = BufReader::new(parent);
        for line in buf_reader.lines() {
            let line = line.unwrap();
            println!("git-push: {}", line);
            if line.find("key fingerprint").is_some() {
                parent.write_all(yes).unwrap();
            }
        }

        false // This is not the child process.
    } else {
        Command::new("git")
            .arg("push")
            .arg("linkd")
            .status()
            .expect("failed to do git push");
        true // This is the child process.
    }
}

fn git_last_commit() -> String {
    let output = Command::new("git")
        .arg("rev-parse")
        .arg("HEAD")
        .output()
        .expect("failed to run git rev-parse");
    String::from_utf8_lossy(&output.stdout).to_string()
}

/// Returns UNIX_TIME in millis.
fn timestamp() -> u128 {
    let now = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap();
    now.as_millis()
}

/// Returns the full path of `bins` manifest file.
fn manifest_path() -> String {
    let package_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/Cargo.toml", package_dir.strip_suffix("/tests").unwrap())
}

/// Returns the full path of `bins/target`.
fn bins_target_dir() -> String {
    let package_dir = env!("CARGO_MANIFEST_DIR");
    format!("{}/target", package_dir.strip_suffix("/tests").unwrap())
}

fn clean_up_known_hosts() {
    // ssh-keygen -f "/home/pi/.ssh/known_hosts" -R "[127.0.0.1]:9987"
    let home_dir = env!("HOME");
    let known_hosts = format!("{}/.ssh/known_hosts", &home_dir);
    let output = Command::new("ssh-keygen")
        .arg("-f")
        .arg(known_hosts)
        .arg("-R")
        .arg("[127.0.0.1]:9987")
        .output()
        .expect("failed to do ssh-keygen");
    println!("ssh-keygen: {:?}", &output);
}
