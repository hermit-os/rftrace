//! This is an ffi wrapper around rftrace-frontend, enabling calling it from c code
//! You can find a usage example in the [repository](https://github.com/tlambertz/rftrace/examples/c)
//! A lot of documentation can be found in the parent workspaces [readme](https://github.com/tlambertz/rftrace).

use rftrace_frontend;

use std::ffi::CStr;
use std::os::raw::c_char;

pub type Events = rftrace_frontend::Events;

#[no_mangle]
/// Wraps rftrace_frontend::enable()
pub unsafe extern "C" fn rftrace_enable() {
    rftrace_frontend::enable();
}

#[no_mangle]
/// Wraps rftrace_frontend::disable();
pub unsafe extern "C" fn rftrace_disable() {
    rftrace_frontend::disable();
}

#[no_mangle]
/// Wraps rftrace_frontend::init();
pub unsafe extern "C" fn rftrace_init(max_event_count: usize, overwriting: bool) -> *mut Events {
    rftrace_frontend::init(max_event_count, overwriting)
}

#[no_mangle]
/// Wraps rftrace_frontend::dump_full_uftrace
pub unsafe extern "C" fn rftrace_dump_full_uftrace(
    events: *mut Events,
    out_dir: *const c_char,
    binary_name: *const c_char,
    linux_mode: u64,
) -> i64 {
    let out_dir = CStr::from_ptr(out_dir).to_string_lossy().into_owned();
    let binary_name = CStr::from_ptr(binary_name).to_string_lossy().into_owned();
    let linux = linux_mode != 0;

    if rftrace_frontend::dump_full_uftrace(&mut *events, &out_dir, &binary_name, linux).is_err()
    {
        return -1;
    }
    0
}

#[no_mangle]
/// Wraps rftrace_frontend::dump_trace
pub unsafe extern "C" fn rftrace_dump_trace(events: *mut Events, outfile: *const c_char) -> i64 {
    let outfile = CStr::from_ptr(outfile).to_string_lossy().into_owned();

    if rftrace_frontend::dump_trace(&mut *events, &outfile).is_err() {
        return -1;
    }
    0
}



#[no_mangle]
pub extern "C" fn marker() -> u64 {
    1337
}