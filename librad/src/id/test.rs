use super::*;
use crate::{keys::device::Key, peer::PeerId};
use lazy_static::lazy_static;
use sodiumoxide::crypto::sign::ed25519::Seed;

const SEED: Seed = Seed([
    20, 21, 6, 102, 102, 57, 20, 67, 219, 198, 236, 108, 148, 15, 182, 52, 167, 27, 29, 81, 181,
    134, 74, 88, 174, 254, 78, 69, 84, 149, 84, 167,
]);
const CREATED_AT: u64 = 1_576_843_598;

fn new_key_from_seed(seed_value: u8) -> Key {
    let mut seed = SEED;
    seed.0[0] = seed_value;
    let created_at = std::time::SystemTime::UNIX_EPOCH
        .checked_add(std::time::Duration::from_secs(CREATED_AT))
        .expect("SystemTime overflow o.O");
    Key::from_seed(&seed, created_at)
}

fn peer_from_key(key: &Key) -> PeerId {
    PeerId::from(key.public())
}

lazy_static! {
    static ref K1: Key = new_key_from_seed(1);
    static ref K2: Key = new_key_from_seed(2);
    static ref K3: Key = new_key_from_seed(3);
    static ref K4: Key = new_key_from_seed(4);
    static ref K5: Key = new_key_from_seed(5);
}

lazy_static! {
    pub static ref D1: PeerId = peer_from_key(&K1);
    pub static ref D2: PeerId = peer_from_key(&K2);
    pub static ref D3: PeerId = peer_from_key(&K3);
    pub static ref D4: PeerId = peer_from_key(&K4);
    pub static ref D5: PeerId = peer_from_key(&K5);
}

struct EmptyResolver {}

impl Resolver<User> for EmptyResolver {
    fn resolve(&self, uri: &RadicleUri) -> Result<User, Error> {
        Err(Error::UserNotPresent(uri.to_owned()))
    }
}

static EMPTY_RESOLVER: EmptyResolver = EmptyResolver {};

struct UserHistory {
    pub revisions: Vec<User>,
}

impl UserHistory {
    pub fn new() -> Self {
        Self { revisions: vec![] }
    }
}

impl RevisionsResolver<User, <std::vec::Vec<User> as IntoIterator>::IntoIter, Vec<User>>
    for UserHistory
{
    fn resolve_revisions(&self, _uri: &RadicleUri) -> std::boxed::Box<Vec<User>> {
        Box::new(self.revisions.iter().rev().cloned().collect())
    }
}

fn new_user(name: &str, devices: &[&'static PeerId]) -> User {
    User::new(name, devices.into_iter().map(|x| *x))
}

#[test]
fn test_user_signatures() {
    // Keep signing the user while adding devices
    let mut user = new_user("foo", &[&*D1]);

    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    let sig1 = user.compute_signature(&K1).unwrap();

    user.devices.insert(D2.to_owned());
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    let sig2 = user.compute_signature(&K1).unwrap();

    assert_ne!(&sig1, &sig2);
}

#[test]
fn test_adding_user_signatures() {
    let mut user = new_user("foo", &[&*D1]);

    // Check that canonical data changes while adding devices
    let data1 = user.canonical_data().unwrap();
    user.devices.insert(D2.to_owned());
    let data2 = user.canonical_data().unwrap();
    user.devices.insert(D3.to_owned());
    let data3 = user.canonical_data().unwrap();
    assert_ne!(&data1, &data2);
    assert_ne!(&data1, &data3);
    assert_ne!(&data2, &data3);

    // Check that canonical data does not change manipulating signatures
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    let data4 = user.canonical_data().unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    let data5 = user.canonical_data().unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    let data6 = user.canonical_data().unwrap();

    assert_eq!(&data3, &data4);
    assert_eq!(&data3, &data5);
    assert_eq!(&data3, &data6);

    // Check signatures collection contents
    assert_eq!(3, user.signatures.len());
    assert!(user.signatures.contains_key(&D1.device_key()));
    assert!(user.signatures.contains_key(&D2.device_key()));
    assert!(user.signatures.contains_key(&D3.device_key()));

    // Check signature verification
    let data = user.canonical_data().unwrap();
    for (k, s) in user.signatures.iter() {
        assert!(s.sig.verify(&data, k));
    }
}

#[test]
fn test_user_verification() {
    // A new user is not valid because no key has signed it
    let mut user = new_user("foo", &[&*D1]);
    assert!(matches!(
        user.check_validity(&EMPTY_RESOLVER),
        Err(Error::SignatureMissing)
    ));
    assert!(!user.is_valid(&EMPTY_RESOLVER));
    // Adding the signature fixes it
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    assert!(matches!(user.check_validity(&EMPTY_RESOLVER), Ok(())));
    assert!(user.is_valid(&EMPTY_RESOLVER));
    // Adding maintainers without signatures invalidates it
    user.devices.insert(D2.to_owned());
    user.devices.insert(D3.to_owned());
    assert!(matches!(user.check_validity(&EMPTY_RESOLVER), Err(_)));
    // Adding the missing signatures does not fix it: D1 signed a previous
    // revision
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    assert!(matches!(
        user.check_validity(&EMPTY_RESOLVER),
        Err(Error::SignatureVerificationFailed)
    ));
    // Cannot sign a project twice with the same key
    assert!(matches!(
        user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER),
        Err(Error::SignatureAlreadyPresent(_))
    ));
    // Removing the signature and re adding it fixes the project
    user.signatures_mut().remove(&K1.public());
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    assert!(user.is_valid(&EMPTY_RESOLVER));
    // Removing a maintainer invalidates it again
    user.devices.remove(&D1);
    assert!(matches!(user.check_validity(&EMPTY_RESOLVER), Err(_)));
}

#[test]
fn test_project_update() {
    // Empty history is invalid
    let mut history = UserHistory::new();
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Err(HistoryVerificationError::EmptyHistory)
    ));

    // History with invalid user is invalid
    let mut user = new_user("foo", &[&*D1]);
    user.revision = 1;
    history.revisions.push(user);

    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Err(HistoryVerificationError::ErrorAtRevision {
            revision: 1,
            error: Error::SignatureMissing,
        })
    ));

    // History with single valid user is valid
    history
        .revisions
        .last_mut()
        .unwrap()
        .sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Ok(())
    ));

    // Adding one device is ok
    let mut user = history.revisions.last().unwrap().clone();
    user.revision = 2;
    user.devices.insert(D2.to_owned());
    user.signatures.clear();
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Ok(())
    ));

    // Adding two devices starting from one is not ok
    history.revisions.pop();
    let mut user = history.revisions.last().unwrap().clone();
    user.revision = 2;
    user.devices.insert(D2.to_owned());
    user.devices.insert(D3.to_owned());
    user.signatures.clear();
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Err(HistoryVerificationError::UpdateError {
            revision: 2,
            error: UpdateVerificationError::NoCurrentQuorum,
        })
    ));

    // Adding two maintainers one by one is ok
    history.revisions.pop();
    let mut user = history.revisions.last().unwrap().clone();
    user.revision = 2;
    user.devices.insert(D2.to_owned());
    user.signatures.clear();
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Ok(())
    ));
    let mut user = history.revisions.last().unwrap().clone();
    user.revision = 3;
    user.devices.insert(D3.to_owned());
    user.signatures.clear();
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K2, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K3, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Ok(())
    ));

    // Changing two devices out of three is not ok
    let mut user = history.revisions.last().unwrap().clone();
    user.revision = 4;
    user.devices.remove(&*D2);
    user.devices.remove(&*D3);
    user.devices.insert(D4.to_owned());
    user.devices.insert(D5.to_owned());
    user.signatures.clear();
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K4, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    user.sign(&K5, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Err(HistoryVerificationError::UpdateError {
            revision: 4,
            error: UpdateVerificationError::NoCurrentQuorum,
        })
    ));

    // Removing two devices out of three is not ok
    history.revisions.pop();
    let mut user = history.revisions.last().unwrap().clone();
    user.revision = 4;
    user.devices.remove(&*D2);
    user.devices.remove(&*D3);
    user.signatures.clear();
    user.sign(&K1, &Signatory::OwnedKey, &EMPTY_RESOLVER)
        .unwrap();
    history.revisions.push(user);
    assert!(matches!(
        User::check_history(&EMPTY_URI, &EMPTY_RESOLVER, &history),
        Err(HistoryVerificationError::UpdateError {
            revision: 4,
            error: UpdateVerificationError::NoPreviousQuorum,
        })
    ));
}
