//! 用系统路由表核实直连下一跳、物理网卡类型与状态，锁定出接口。
use super::Route;
use socket2::Socket;
use std::{io, net::Ipv4Addr, os::windows::io::AsRawSocket, ptr};
use windows_sys::Win32::NetworkManagement::IpHelper::{
    GetBestRoute2, GetIfEntry2, MIB_IF_ROW2, MIB_IPFORWARD_ROW2,
};
use windows_sys::Win32::Networking::WinSock::{
    AF_INET, IP_UNICAST_IF, IPPROTO_IP, SOCKADDR_IN, SOCKADDR_INET, setsockopt,
};

pub(super) fn resolve(ip: Ipv4Addr) -> Option<Route> {
    let mut dest = SOCKADDR_IN {
        sin_family: AF_INET,
        ..Default::default()
    };
    dest.sin_addr.S_un.S_addr = u32::from_ne_bytes(ip.octets());
    let dest = SOCKADDR_INET { Ipv4: dest };
    let mut row = MIB_IPFORWARD_ROW2::default();
    let mut source = SOCKADDR_INET::default();
    unsafe {
        if GetBestRoute2(ptr::null(), 0, ptr::null(), &dest, 0, &mut row, &mut source) != 0
            || source.si_family != AF_INET
            || row.NextHop.si_family != AF_INET
            || row.NextHop.Ipv4.sin_addr.S_un.S_addr != 0
            || row.DestinationPrefix.PrefixLength == 0
        {
            return None;
        }
        let mut interface = MIB_IF_ROW2 {
            InterfaceLuid: row.InterfaceLuid,
            ..Default::default()
        };
        if GetIfEntry2(&mut interface) != 0
            || !matches!(interface.Type, 6 | 71)
            || interface.OperStatus != 1
            || interface.InterfaceAndOperStatusFlags._bitfield & 1 == 0
        {
            return None;
        }
        Some(Route {
            local: Ipv4Addr::from(source.Ipv4.sin_addr.S_un.S_addr.to_ne_bytes()),
            network: row.InterfaceIndex.into(),
        })
    }
}

pub(super) fn pin(socket: &Socket, index: u32) -> io::Result<()> {
    let bytes = index.to_be_bytes();
    if unsafe {
        setsockopt(
            socket.as_raw_socket() as _,
            IPPROTO_IP,
            IP_UNICAST_IF,
            bytes.as_ptr(),
            4,
        )
    } != 0
    {
        return Err(io::Error::last_os_error());
    }
    Ok(())
}
