use rftrace_frontend;

use std::ffi::CStr;
use std::os::raw::c_char;

pub type Events = rftrace_frontend::Events;

#[no_mangle]
pub unsafe extern "C" fn rftrace_enable() {
    rftrace_frontend::enable();
}

#[no_mangle]
pub unsafe extern "C" fn rftrace_disable() {
    rftrace_frontend::disable();
}

#[no_mangle]
pub unsafe extern "C" fn rftrace_init(max_event_count: usize, overwriting: bool) -> *mut Events {
    let events = rftrace_frontend::init(max_event_count, overwriting);
    events
}

#[no_mangle]
pub unsafe extern "C" fn rftrace_dump_full_uftrace(
    events: *mut Events,
    out_dir: *const c_char,
    binary_name: *const c_char,
    linux_mode: u64,
) -> i64 {
    let out_dir = CStr::from_ptr(out_dir).to_string_lossy().into_owned();
    let binary_name = CStr::from_ptr(binary_name).to_string_lossy().into_owned();
    let linux = linux_mode != 0;

    if let Err(_) = rftrace_frontend::dump_full_uftrace(&mut *events, &out_dir, &binary_name, linux)
    {
        return -1;
    }
    return 0;
}

#[no_mangle]
pub unsafe extern "C" fn rftrace_dump_trace(events: *mut Events, outfile: *const c_char) -> i64 {
    let outfile = CStr::from_ptr(outfile).to_string_lossy().into_owned();

    if let Err(_) = rftrace_frontend::dump_trace(&mut *events, &outfile) {
        return -1;
    }
    return 0;
}
