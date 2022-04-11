use std::collections::HashMap;

// A history containing two entries
pub fn random_history(name: &'static str) -> cob::History {
    let mut backend = automerge::Backend::new();
    let mut frontend = automerge::Frontend::new();
    let (_, root_change) = frontend
        .change::<_, _, automerge::InvalidChangeRequest>(None, |d| {
            d.add_change(automerge::LocalChange::set(
                automerge::Path::root().key("name"),
                automerge::Value::Primitive(automerge::Primitive::Str(name.into())),
            ))?;
            Ok(())
        })
        .unwrap();
    let (patch, root_change) = backend.apply_local_change(root_change.unwrap()).unwrap();
    frontend.apply_patch(patch).unwrap();
    let (_, second_change) = frontend
        .change::<_, _, automerge::InvalidChangeRequest>(None, |d| {
            d.add_change(automerge::LocalChange::set(
                automerge::Path::root().key("name2"),
                automerge::Value::Primitive(automerge::Primitive::Str(name.into())),
            ))?;
            Ok(())
        })
        .unwrap();
    let root_change = root_change.clone();
    let (_, second_change) = backend.apply_local_change(second_change.unwrap()).unwrap();
    let second_entry = cob::HistoryEntry::new(
        random_oid(),
        random_urn(),
        Vec::<git2::Oid>::new(),
        cob::EntryContents::Automerge(second_change.raw_bytes().to_vec()),
    );
    let root_entry = cob::HistoryEntry::new(
        random_oid(),
        random_urn(),
        vec![second_entry.id().clone()],
        cob::EntryContents::Automerge(root_change.raw_bytes().to_vec()),
    );
    let mut entries = HashMap::new();
    entries.insert(root_entry.id().clone(), root_entry.clone());
    entries.insert(second_entry.id().clone(), second_entry);
    cob::History::new(root_entry.id().clone(), entries).unwrap()
}

pub fn random_oid() -> git2::Oid {
    let oid_raw: [u8; 20] = rand::random();
    git2::Oid::from_bytes(&oid_raw).unwrap()
}

pub fn random_urn() -> link_identities::git::Urn {
    radicle_git_ext::Oid::from(random_oid()).into()
}
