// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use rusty_fork::rusty_fork_test;

use lnk_exe::cli::args::*;

#[test]
fn lnk_profile_first_precedence() {
    let external = vec![
        "xxx".to_string(),
        LNK_PROFILE_ARG.to_string(),
        "def".to_string(),
    ];
    let args = Args {
        global: Global {
            lnk_profile: Some("abc".parse().unwrap()),
            lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_PROFILE_ARG, &external);
        assert_eq!("abc", external[index.unwrap() + 1]);
    }
}

#[test]
fn lnk_profile_first_precedence_multiple_externals() {
    let external = vec![
        "xxx".to_string(),
        LNK_PROFILE_ARG.to_string(),
        "def".to_string(),
        LNK_PROFILE_ARG.to_string(),
        "ghi".to_string(),
    ];
    let args = Args {
        global: Global {
            lnk_profile: Some("abc".parse().unwrap()),
            lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_PROFILE_ARG, &external);
        assert_eq!("abc", external[index.unwrap() + 1]);
    }
}

// N.B. we fork these tests into subprocesses since they modify environment
// variable, notably RAD_PROFILE, which can affect other tests running.
rusty_fork_test! {
#[test]
fn lnk_profile_second_precedence() {
    env::set_var("LNK_PROFILE", "ghi");
    let external = vec![
        "xxx".to_string(),
        LNK_PROFILE_ARG.to_string(),
        "def".to_string(),
    ];
    let args = Args {
        global: Global {
            lnk_profile: None,
        lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_PROFILE_ARG, &external);
        assert_eq!("def", external[index.unwrap() + 1]);
    }
}

#[test]
fn lnk_profile_second_precedence_multiple() {
    env::set_var("LNK_PROFILE", "ghi");
    let external = vec![
        "xxx".to_string(),
        LNK_PROFILE_ARG.to_string(),
        "def".to_string(),
        LNK_PROFILE_ARG.to_string(),
        "ghi".to_string(),
    ];
    let args = Args {
        global: Global {
            lnk_profile: None,
        lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_PROFILE_ARG, &external);
        assert_eq!("ghi", external[index.unwrap() + 1]);
    }
}

#[test]
fn lnk_profile_env_var() {
    env::set_var("LNK_PROFILE", "ghi");
    let external = vec!["xxx".to_string()];
    let args = Args {
        global: Global {
            lnk_profile: None,
        lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_PROFILE_ARG, &external);
        assert_eq!("ghi", external[index.unwrap() + 1]);
    }
}

#[test]
fn lnk_verbose_second_precedence() {
    env::set_var("LNK_PROFILE", "ghi");
    let external = vec!["xxx".to_string(), LNK_VERBOSE_ARG.to_string()];
    let args = Args {
        global: Global {
            lnk_profile: None,
        lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_VERBOSE_ARG, &external);
        assert!(index.is_some());
    }
}

#[test]
fn lnk_verbose_env_var() {
    env::set_var("LNK_VERBOSE", "1");
    let external = vec!["xxx".to_string()];
    let args = Args {
        global: Global {
            lnk_profile: None,
        lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_VERBOSE_ARG, &external);
        assert!(index.is_some());
    }
}
/* end rusty_fork! */
}

#[test]
fn lnk_verbose_first_precedence() {
    let external = vec!["xxx".to_string()];
    let args = Args {
        global: Global {
            lnk_profile: Some("abc".parse().unwrap()),
            lnk_ssh_auth_sock: Default::default(),
            lnk_quiet: false,
            lnk_verbose: true,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(LNK_VERBOSE_ARG, &external);
        assert!(index.is_some());
    }
}
