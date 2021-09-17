// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::env;

use rusty_fork::rusty_fork_test;

use rad_exe::cli::args::*;

#[test]
fn rad_profile_first_precedence() {
    let external = vec![
        "xxx".to_string(),
        RAD_PROFILE_ARG.to_string(),
        "def".to_string(),
    ];
    let args = Args {
        global: Global {
            rad_profile: Some("abc".parse().unwrap()),
            rad_quiet: false,
            rad_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_PROFILE_ARG, &external);
        assert_eq!("abc", external[index.unwrap() + 1]);
    }
}

#[test]
fn rad_profile_first_precedence_multiple_externals() {
    let external = vec![
        "xxx".to_string(),
        RAD_PROFILE_ARG.to_string(),
        "def".to_string(),
        RAD_PROFILE_ARG.to_string(),
        "ghi".to_string(),
    ];
    let args = Args {
        global: Global {
            rad_profile: Some("abc".parse().unwrap()),
            rad_quiet: false,
            rad_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_PROFILE_ARG, &external);
        assert_eq!("abc", external[index.unwrap() + 1]);
    }
}

// N.B. we fork these tests into subprocesses since they modify environment
// variable, notably RAD_PROFILE, which can affect other tests running.
rusty_fork_test! {
#[test]
fn rad_profile_second_precedence() {
    env::set_var("RAD_PROFILE", "ghi");
    let external = vec![
        "xxx".to_string(),
        RAD_PROFILE_ARG.to_string(),
        "def".to_string(),
    ];
    let args = Args {
        global: Global {
            rad_profile: None,
            rad_quiet: false,
            rad_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_PROFILE_ARG, &external);
        assert_eq!("def", external[index.unwrap() + 1]);
    }
}

#[test]
fn rad_profile_second_precedence_multiple() {
    env::set_var("RAD_PROFILE", "ghi");
    let external = vec![
        "xxx".to_string(),
        RAD_PROFILE_ARG.to_string(),
        "def".to_string(),
        RAD_PROFILE_ARG.to_string(),
        "ghi".to_string(),
    ];
    let args = Args {
        global: Global {
            rad_profile: None,
            rad_quiet: false,
            rad_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_PROFILE_ARG, &external);
        assert_eq!("ghi", external[index.unwrap() + 1]);
    }
}

#[test]
fn rad_profile_env_var() {
    env::set_var("RAD_PROFILE", "ghi");
    let external = vec!["xxx".to_string()];
    let args = Args {
        global: Global {
            rad_profile: None,
            rad_quiet: false,
            rad_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_PROFILE_ARG, &external);
        assert_eq!("ghi", external[index.unwrap() + 1]);
    }
}

#[test]
fn rad_verbose_second_precedence() {
    env::set_var("RAD_PROFILE", "ghi");
    let external = vec!["xxx".to_string(), RAD_VERBOSE_ARG.to_string()];
    let args = Args {
        global: Global {
            rad_profile: None,
            rad_quiet: false,
            rad_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_VERBOSE_ARG, &external);
        assert!(index.is_some());
    }
}

#[test]
fn rad_verbose_env_var() {
    env::set_var("RAD_VERBOSE", "1");
    let external = vec!["xxx".to_string()];
    let args = Args {
        global: Global {
            rad_profile: None,
            rad_quiet: false,
            rad_verbose: false,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_VERBOSE_ARG, &external);
        assert!(index.is_some());
    }
}
/* end rusty_fork! */
}

#[test]
fn rad_verbose_first_precedence() {
    let external = vec!["xxx".to_string()];
    let args = Args {
        global: Global {
            rad_profile: Some("abc".parse().unwrap()),
            rad_quiet: false,
            rad_verbose: true,
        },
        command: Command::External(external),
    };

    let args = sanitise_globals(args);
    if let Command::External(external) = args.command {
        let index = find_arg(RAD_VERBOSE_ARG, &external);
        assert!(index.is_some());
    }
}
