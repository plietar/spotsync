extern crate libc;
use libc::c_int;
use libc::c_char;
use libc::timeval;
extern {
    pub fn msntp_start_server(port: c_int) -> c_int;
    pub fn msntp_serve() -> c_int;
    pub fn msntp_get_offset(hostname: *const c_char, port: c_int, offset: *mut timeval);
}

