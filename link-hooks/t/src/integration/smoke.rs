// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

//! These tests rely on the executables found in `test/hooks`. There are three
//! executables:
//!   * `echo-data` - parses `Data` and writes it to the file path passed as an
//!     argument.
//!   * `echo-track` - parses `Track` and writes it to the file path passed as
//!     an argument.
//!   * `echo-forever` - hangs for 10s to ensure other hooks continue
//!     processing.

use std::{
    io::Read as _,
    iter,
    path::{Path, PathBuf},
};

use link_hooks::{
    hook::{self, Hook, Process as _},
    Data,
    Hooks,
    Notification,
    Track,
};
use radicle_git_ext::Oid;
use tempfile::NamedTempFile;
use test_helpers::logging;
use tokio::process::Child;

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_echo_hooks() {
    logging::init();

    let data_hook_path = setup_hook("data");
    let track_hook_path = setup_hook("track");
    let mut data_out = NamedTempFile::new().unwrap();
    let mut track_out = NamedTempFile::new().unwrap();
    let data_hooks = vec![Hook::<Child>::spawn(
        data_hook_path,
        Some(format!("{}", data_out.path().display())),
    )
    .await
    .unwrap()];
    let track_hooks = vec![Hook::<Child>::spawn(
        track_hook_path,
        Some(format!("{}", track_out.path().display())),
    )
    .await
    .unwrap()];

    let hooks = Hooks::new(hook::Config::default(), data_hooks, track_hooks);
    assert_notifications(hooks, &mut data_out, &mut track_out).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_hanging_hook() {
    logging::init();

    let data_hook_path = setup_hook("data");
    let track_hook_path = setup_hook("track");
    let forever_hook_path = setup_hook("forever");
    let mut data_out = NamedTempFile::new().unwrap();
    let mut track_out = NamedTempFile::new().unwrap();
    let data_hooks = vec![
        Hook::<Child>::spawn(
            data_hook_path,
            Some(format!("{}", data_out.path().display())),
        )
        .await
        .unwrap(),
        Hook::<Child>::spawn(forever_hook_path, None::<String>)
            .await
            .unwrap(),
    ];
    let track_hooks = vec![Hook::<Child>::spawn(
        track_hook_path,
        Some(format!("{}", track_out.path().display())),
    )
    .await
    .unwrap()];

    let hooks = Hooks::new(hook::Config::default(), data_hooks, track_hooks);
    assert_notifications(hooks, &mut data_out, &mut track_out).await
}

async fn assert_notifications(
    hooks: Hooks<Child>,
    data_out: &mut NamedTempFile,
    track_out: &mut NamedTempFile,
) {
    let notifications = vec![
        "rad:git:hnrkyzfpih4pqsw3cp1donkmwsgh9w5fwfdwo/refs/heads/main 0c3b4502a83a309b19123adc60a23e4e92bb13fb aeff7e8e964c47ba67a0c6eeba3beb62e29379d4\n".parse::<Data<Oid>>().unwrap().into(),
        "rad:git:hnrkyzfpih4pqsw3cp1donkmwsgh9w5fwfdwo hyyqpngdoe4x4oto3emfdppbw7sj1pfaghbpmmhz5rqiuqg8uofmeo 0c3b4502a83a309b19123adc60a23e4e92bb13fb aeff7e8e964c47ba67a0c6eeba3beb62e29379d4\n".parse::<Track<Oid>>().unwrap().into(),
        "rad:git:hnrkyzfpih4pqsw3cp1donkmwsgh9w5fwfdwo default 0c3b4502a83a309b19123adc60a23e4e92bb13fb aeff7e8e964c47ba67a0c6eeba3beb62e29379d4\n".parse::<Track<Oid>>().unwrap().into(),
        ];

    hooks
        .run(futures::stream::iter(notifications.clone()))
        .await;

    let expected = {
        let mut buf = String::new();
        data_out.read_to_string(&mut buf).unwrap();
        let expected = iter::once(Notification::from(buf.parse::<Data<Oid>>().unwrap()));

        let mut buf = String::new();
        track_out.read_to_string(&mut buf).unwrap();
        expected
            .chain(buf.split('\n').filter_map(|track| {
                if !track.is_empty() {
                    let mut track = track.to_owned();
                    track.push('\n');
                    Some(Notification::from(track.parse::<Track<Oid>>().unwrap()))
                } else {
                    None
                }
            }))
            .collect::<Vec<_>>()
    };

    assert_eq!(notifications, expected);
}

fn setup_hook(hook: &str) -> PathBuf {
    let test_path = Path::new(env!("CARGO_MANIFEST_DIR"));
    let root = test_path
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .join("test/hooks");
    let manifest = root.join(format!("echo-{}", hook)).join("Cargo.toml");
    let hook_path = root
        .join("target")
        .join("debug")
        .join(format!("echo-{}", hook));

    if !hook_path.exists() {
        let out = std::process::Command::new("cargo")
            .args(&[
                "build",
                "--bin",
                &format!("echo-{}", hook),
                "--manifest-path",
                &format!("{}", manifest.display()),
            ])
            .output()
            .unwrap();
        if !out.status.success() {
            println!("{:#?}", out)
        }
    }

    hook_path
}
