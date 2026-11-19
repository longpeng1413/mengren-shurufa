//! 默认网络由 ConnectivityManager 回调提供。内核网络句柄固定单个 socket，不改变输入法进程网络。
use super::Route;
use serde::Deserialize;
use socket2::Socket;
use std::net::Ipv4Addr;
use std::os::fd::AsRawFd;
use std::{io, sync::Mutex};

#[derive(Deserialize, Default, PartialEq)]
struct Network {
    handle: u64,
    local: Vec<Ipv4Addr>,
    routes: Vec<(Ipv4Addr, u8, bool)>,
}

static NETWORK: Mutex<Option<Network>> = Mutex::new(None);

pub fn update(json: &str) {
    *crate::service::lock(&NETWORK) = serde_json::from_str(json).ok();
}

pub(super) fn resolve(ip: Ipv4Addr) -> Option<Route> {
    let state = crate::service::lock(&NETWORK);
    let net = state.as_ref()?;
    let (dest, prefix, direct) = net
        .routes
        .iter()
        .filter(|(address, bits, _)| *bits <= 32 && ip_matches(ip, *address, *bits))
        .max_by_key(|(_, bits, _)| *bits)?;
    if !direct || *prefix == 0 || net.handle == 0 {
        return None;
    }
    Some(Route {
        local: *net.local.iter().find(|a| ip_matches(**a, *dest, *prefix))?,
        network: net.handle,
    })
}

fn ip_matches(a: Ipv4Addr, b: Ipv4Addr, bits: u8) -> bool {
    (u32::from(a) ^ u32::from(b)).leading_zeros() >= u32::from(bits)
}

pub(super) fn pin(socket: &Socket, network: u64) -> io::Result<()> {
    #[link(name = "android")]
    unsafe extern "C" {
        fn android_setsocknetwork(network: u64, fd: i32) -> i32;
    }
    if unsafe { android_setsocknetwork(network, socket.as_raw_fd()) } != 0 {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
