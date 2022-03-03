// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPL-3.0-or-later

use git_ref_format::{component, name, refname, refspec, Error, Qualified, RefStr, RefString};

#[test]
fn refname_macro_works() {
    assert_eq!("refs/heads/main", refname!("refs/heads/main").as_str())
}

#[test]
fn component_macro_works() {
    assert_eq!("self", name::component!("self").as_str())
}

#[test]
fn pattern_macro_works() {
    assert_eq!("refs/heads/*", refspec::pattern!("refs/heads/*").as_str())
}

#[test]
fn empty() {
    assert_matches!(RefStr::try_from_str(""), Err(Error::Empty));
    assert_matches!(RefString::try_from("".to_owned()), Err(Error::Empty));
}

#[test]
fn join() {
    let s = name::REFS.join(name::HEADS);
    let t = s.join(name::MAIN);
    assert_eq!("refs/heads", s.as_str());
    assert_eq!("refs/heads/main", t.as_str());
}

#[test]
fn join_and() {
    assert_eq!(
        "refs/heads/this/that",
        name::REFS
            .join(name::HEADS)
            .and(refname!("this"))
            .and(refname!("that"))
            .as_str()
    )
}

#[test]
fn strip_prefix() {
    assert_eq!(
        "main",
        name::REFS_HEADS_MAIN
            .strip_prefix(refname!("refs/heads"))
            .unwrap()
            .as_str()
    )
}

#[test]
fn strip_prefix_not_prefix() {
    assert!(name::REFS_HEADS_MAIN
        .strip_prefix(refname!("refs/tags"))
        .is_none())
}

#[test]
fn qualified() {
    assert_eq!(
        "refs/heads/main",
        name::REFS_HEADS_MAIN.qualified().unwrap().as_str()
    )
}

#[test]
fn qualified_tag() {
    assert_eq!(
        "refs/tags/v1",
        refname!("refs/tags/v1").qualified().unwrap().as_str()
    )
}

#[test]
fn qualified_remote_tracking() {
    assert_eq!(
        "refs/remotes/origin/master",
        refname!("refs/remotes/origin/master")
            .qualified()
            .unwrap()
            .as_str()
    )
}

#[test]
fn not_qualified() {
    assert!(name::MAIN.qualified().is_none())
}

#[test]
fn qualified_from_components() {
    assert_eq!(
        "refs/heads/main",
        Qualified::from_components(component::HEADS, component::MAIN, None).as_str()
    )
}

#[test]
fn qualified_from_components_with_iter() {
    assert_eq!(
        "refs/heads/foo/bar/baz",
        Qualified::from_components(
            component::HEADS,
            name::component!("foo"),
            [name::component!("bar"), name::component!("baz")]
        )
        .as_str()
    )
}

#[test]
fn qualified_from_components_non_empty_iter() {
    let q = Qualified::from_components(component::HEADS, component::MAIN, None);
    let (refs, heads, main, mut empty) = q.non_empty_iter();
    assert!(empty.next().is_none());
    assert_eq!(("refs", "heads", "main"), (refs, heads, main))
}

#[test]
fn qualified_from_components_non_empty_components() {
    let q = Qualified::from_components(component::HEADS, component::MAIN, Some(component::MASTER));
    let (refs, heads, main, mut master) = q.non_empty_components();
    assert_eq!(
        (
            component::REFS,
            component::HEADS,
            component::MAIN,
            component::MASTER
        ),
        (refs, heads, main, master.next().unwrap())
    )
}

#[test]
fn namespaced() {
    assert_eq!(
        "refs/namespaces/foo/refs/heads/main",
        refname!("refs/namespaces/foo/refs/heads/main")
            .namespaced()
            .unwrap()
            .as_str()
    )
}

#[test]
fn not_namespaced() {
    assert!(name::REFS_HEADS_MAIN.namespaced().is_none())
}

#[test]
fn not_namespaced_because_not_qualified() {
    assert!(refname!("refs/namespaces/foo/banana")
        .namespaced()
        .is_none())
}

#[test]
fn strip_namespace() {
    assert_eq!(
        "refs/rad/id",
        refname!("refs/namespaces/xyz/refs/rad/id")
            .namespaced()
            .unwrap()
            .strip_namespace()
            .as_str()
    )
}

#[test]
fn strip_nested_namespaces() {
    let full = refname!("refs/namespaces/a/refs/namespaces/b/refs/heads/main");
    let namespaced = full.namespaced().unwrap();
    let strip_first = namespaced.strip_namespace();
    let nested = strip_first.namespaced().unwrap();
    let strip_second = nested.strip_namespace();

    assert_eq!("a", namespaced.namespace().as_str());
    assert_eq!("b", nested.namespace().as_str());
    assert_eq!("refs/namespaces/b/refs/heads/main", strip_first.as_str());
    assert_eq!("refs/heads/main", strip_second.as_str());
}

#[test]
fn add_namespace() {
    assert_eq!(
        "refs/namespaces/foo/refs/heads/main",
        name::REFS_HEADS_MAIN
            .qualified()
            .unwrap()
            .add_namespace(refname!("foo").head())
            .as_str()
    )
}

#[test]
fn iter() {
    assert_eq!(
        vec!["refs", "heads", "main"],
        name::REFS_HEADS_MAIN.iter().collect::<Vec<_>>()
    )
}

#[test]
fn push_pop() {
    let mut s = name::REFS.to_owned();
    s.push(name::HEADS);
    s.push(name::MAIN);

    assert_eq!("refs/heads/main", s.as_str());
    assert!(s.pop());
    assert!(s.pop());
    assert_eq!("refs", s.as_str());
    assert!(!s.pop());
    assert_eq!("refs", s.as_str());
}

#[test]
fn to_pattern() {
    assert_eq!(
        "refs/heads/*",
        refname!("refs/heads")
            .to_pattern(refspec::pattern!("*"))
            .as_str()
    )
}

#[test]
fn with_pattern() {
    assert_eq!(
        "refs/heads/*",
        refname!("refs/heads").with_pattern(refspec::STAR).as_str()
    )
}

#[test]
fn with_pattern_and() {
    assert_eq!(
        "refs/*/heads",
        refname!("refs")
            .with_pattern(refspec::STAR)
            .and(name::HEADS)
            .as_str()
    )
}

#[test]
fn collect() {
    assert_eq!(
        "refs/heads/main",
        IntoIterator::into_iter([name::REFS, name::HEADS, name::MAIN])
            .collect::<RefString>()
            .as_str()
    )
}

#[test]
fn collect_components() {
    let a = name::REFS_HEADS_MAIN.to_owned();
    let b = a.components().collect();
    assert_eq!(a, b)
}

#[test]
fn collect_pattern_duplicate_glob() {
    assert_matches!(
        IntoIterator::into_iter([
            refspec::Component::Normal(name::REFS),
            refspec::Component::Glob(None),
            refspec::Component::Glob(Some(refspec::pattern!("fo*").as_ref()))
        ])
        .collect::<Result<_, _>>(),
        Err(refspec::DuplicateGlob)
    )
}
