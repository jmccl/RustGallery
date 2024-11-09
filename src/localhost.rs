use ngx::ffi:: { ngx_connection_local_sockaddr, ngx_int_t, ngx_sock_ntop, sockaddr, sockaddr_storage, INET_ADDRSTRLEN };

use ngx::{ core, http };

use std::os::raw::c_int;

pub fn is_localhost(request: &mut http::Request) -> bool {
    unsafe {
        return get_client_ip(request) == "127.0.0.1";
    }
}

/**
 * See the httporigdst.rs example in the ngx-rust crate.
 * This only works for IPv4.
 */
unsafe fn get_client_ip(request: &mut http::Request) -> String {
    let c = request.connection();

    if (*c).type_ != libc::SOCK_STREAM {
        return String::new();
    }

    if ngx_connection_local_sockaddr(c, std::ptr::null_mut(), 0) != <core::Status as Into<ngx_int_t>>::into(core::Status::NGX_OK) {    
        return String::new();
    }

    let level: c_int;
    let optname: c_int;
    match (*(*c).local_sockaddr).sa_family as i32 {
        libc::AF_INET => {
            level = libc::SOL_IP;
            optname = libc::SO_ORIGINAL_DST;
        }
        _ => {
            // Doesn't work with ipv6
            return String::new();
        }
    }

    let mut addr: sockaddr_storage = { std::mem::zeroed() };
    let mut addrlen: libc::socklen_t = std::mem::size_of_val(&addr) as libc::socklen_t;
    let rc = libc::getsockopt(
        (*c).fd,
        level,
        optname,
        &mut addr as *mut _ as *mut _,
        &mut addrlen as *mut u32,
    );
    if rc == -1 {
        return String::new();
    }
    let mut ip: Vec<u8> = vec![0; INET_ADDRSTRLEN as usize];
    let e = unsafe {
        ngx_sock_ntop(
            std::ptr::addr_of_mut!(addr) as *mut sockaddr,
            std::mem::size_of::<sockaddr>() as u32,
            ip.as_mut_ptr(),
            INET_ADDRSTRLEN as usize,
            0,
        )
    };
    if e == 0 {
        return String::new();
    }
    ip.truncate(e);

    String::from_utf8(ip).unwrap()
}
