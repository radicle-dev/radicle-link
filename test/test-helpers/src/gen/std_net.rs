// Copyright © 2022 The Radicle Link Contributors
// SPDX-License-Identifier: GPLv3-or-later

use std::net::{Ipv4Addr, Ipv6Addr, SocketAddr, SocketAddrV4, SocketAddrV6};

use proptest::prelude::*;

prop_compose! {
    pub fn gen_ipaddr_v4()
        (a in any::<u8>(),
         b in any::<u8>(),
         c in any::<u8>(),
         d in any::<u8>()) -> Ipv4Addr{
        Ipv4Addr::new(a, b, c, d)
    }
}

prop_compose! {
    pub fn gen_ipaddr_v6()
        (a in any::<u16>(),
         b in any::<u16>(),
         c in any::<u16>(),
         d in any::<u16>(),
         e in any::<u16>(),
         f in any::<u16>(),
         g in any::<u16>(),
         h in any::<u16>()) -> Ipv6Addr
    {
        Ipv6Addr::new(a, b, c, d, e, f, g, h)
    }
}

pub fn gen_socket_v4() -> impl Strategy<Value = SocketAddr> {
    any::<u16>().prop_flat_map(move |port| {
        gen_ipaddr_v4().prop_map(move |v4| SocketAddr::V4(SocketAddrV4::new(v4, port)))
    })
}

pub fn gen_socket_v6() -> impl Strategy<Value = SocketAddr> {
    any::<u16>().prop_flat_map(move |port| {
        gen_ipaddr_v6().prop_map(move |v6| SocketAddr::V6(SocketAddrV6::new(v6, port, 0, 0)))
    })
}

pub fn gen_socket_addr() -> impl Strategy<Value = SocketAddr> {
    prop_oneof![gen_socket_v4(), gen_socket_v6()]
}
