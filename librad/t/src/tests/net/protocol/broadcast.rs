// Copyright © 2021 The Radicle Link Contributors
//
// This file is part of radicle-link, distributed under the GPLv3 with Radicle
// Linking Exception. For full terms see the included LICENSE file.

use std::{
    collections::hash_map::DefaultHasher,
    hash::{Hash, Hasher},
    iter,
};

use librad::{
    net::protocol::{broadcast, PeerAdvertisement, PeerInfo},
    PeerId,
    SecretKey,
};
use once_cell::sync::Lazy;
use test_helpers::roundtrip;

#[derive(Clone, Debug, PartialEq, minicbor::Encode, minicbor::Decode)]
enum MessageV1<Addr, Payload> {
    #[n(0)]
    #[cbor(array)]
    Have {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        val: Payload,
    },

    #[n(1)]
    #[cbor(array)]
    Want {
        #[n(0)]
        origin: PeerInfo<Addr>,
        #[n(1)]
        val: Payload,
    },
}
static ORIGIN: Lazy<PeerInfo<()>> = Lazy::new(|| PeerInfo {
    peer_id: PeerId::from(SecretKey::new()),
    advertised_info: PeerAdvertisement {
        listen_addrs: iter::empty().into(),
        capabilities: Default::default(),
    },
    seen_addrs: iter::empty().into(),
});

#[test]
fn roundtrip_message() {
    let have = broadcast::Message::<(), ()>::Have {
        origin: ORIGIN.clone(),
        val: (),
        ext: Some(broadcast::Ext::default()),
    };

    roundtrip::cbor(have)
}

#[test]
fn backwards_compat_v1() {
    let v2 = broadcast::Message::<(), ()>::Have {
        origin: ORIGIN.clone(),
        val: (),
        ext: Some(broadcast::Ext::default().next_hop()),
    };
    let v1: MessageV1<(), ()> = minicbor::decode(&minicbor::to_vec(&v2).unwrap()).unwrap();

    assert_eq!(
        v1,
        MessageV1::Have {
            origin: ORIGIN.clone(),
            val: ()
        }
    );
}

#[test]
fn forwards_compat_v2() {
    let v1 = MessageV1::<(), ()>::Have {
        origin: ORIGIN.clone(),
        val: (),
    };
    let v2: broadcast::Message<(), ()> = minicbor::decode(&minicbor::to_vec(&v1).unwrap()).unwrap();

    assert_eq!(
        v2,
        broadcast::Message::Have {
            origin: ORIGIN.clone(),
            val: (),
            ext: None,
        }
    )
}

#[test]
fn message_id() {
    let a = broadcast::Message::Have {
        origin: ORIGIN.clone(),
        val: 'a',
        ext: Some(broadcast::Ext::default()),
    };
    let hashed = hash(&a);

    for x in [
        // new seqno
        broadcast::Message::Have {
            origin: a.origin().clone(),
            val: *a.payload(),
            ext: Some(broadcast::Ext::default()),
        },
        // different origin peer
        broadcast::Message::Have {
            origin: PeerInfo {
                peer_id: PeerId::from(SecretKey::new()),
                ..ORIGIN.clone()
            },
            val: *a.payload(),
            ext: a.ext().cloned(),
        },
        // different variant
        broadcast::Message::Want {
            origin: a.origin().clone(),
            val: *a.payload(),
            ext: a.ext().cloned(),
        },
    ] {
        assert_ne!(hashed, hash(&x));
    }

    for y in [
        // different payload
        broadcast::Message::Have {
            origin: a.origin().clone(),
            val: 'b',
            ext: a.ext().cloned(),
        },
        // different hop count
        broadcast::Message::Have {
            origin: a.origin().clone(),
            val: *a.payload(),
            ext: a.ext().map(|x| x.clone().next_hop()),
        },
    ] {
        assert_eq!(hashed, hash(&y))
    }

    let b = broadcast::Message::Want {
        origin: ORIGIN.clone(),
        val: 'b',
        ext: None,
    };
    let hashed = hash(&b);
    for z in [
        // different payload
        broadcast::Message::Want {
            origin: b.origin().clone(),
            val: 'c',
            ext: None,
        },
        // some ext
        broadcast::Message::Want {
            origin: b.origin().clone(),
            val: *b.payload(),
            ext: Some(broadcast::Ext::default()),
        },
    ] {
        assert_ne!(hashed, hash(&z))
    }
}

fn hash<T: Hash>(t: &T) -> u64 {
    let mut s = DefaultHasher::new();
    t.hash(&mut s);
    s.finish()
}
