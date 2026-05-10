//! This is an ffi wrapper around rftrace-frontend, enabling calling it from c code
//! You can find a usage example in the [repository](https://github.com/hermit-os/rftrace/examples/c)
//! A lot of documentation can be found in the parent workspaces [readme](https://github.com/hermit-os/rftrace).

// FIXME:
#![expect(clippy::missing_safety_doc)]

use std::ffi::CStr;
use std::os::raw::c_char;
use std::path::Path;

pub type Events = rftrace_frontend::Events;

#[no_mangle]
/// Wraps [`rftrace_frontend::enable`].
pub unsafe extern "C" fn rftrace_enable() {
    rftrace_frontend::enable();
}

#[no_mangle]
/// Wraps [`rftrace_frontend::disable`].
pub unsafe extern "C" fn rftrace_disable() {
    rftrace_frontend::disable();
}

#[no_mangle]
/// Wraps [`rftrace_frontend::init`].
pub unsafe extern "C" fn rftrace_init(max_event_count: usize, overwriting: bool) -> *mut Events {
    rftrace_frontend::init(max_event_count, overwriting)
}

fn try_path_from_c_str(c_str: &CStr) -> Option<&Path> {
    let bytes = c_str.to_bytes();

    cfg_select! {
        unix => {
            use std::os::unix::ffi::OsStrExt;
            use std::ffi::OsStr;

            Some(OsStr::from_bytes(bytes).as_ref())
        }
        _ => {
            str::from_utf8(bytes).ok().map(|s| s.as_ref())
        }
    }
}

#[no_mangle]
/// Wraps [`rftrace_frontend::dump_full_uftrace`].
pub unsafe extern "C" fn rftrace_dump_full_uftrace(
    events: *mut Events,
    out_dir: *const c_char,
    binary_name: *const c_char,
) -> i64 {
    let Some(out_dir) = try_path_from_c_str(CStr::from_ptr(out_dir)) else {
        return -1;
    };
    let binary_name = CStr::from_ptr(binary_name).to_string_lossy().into_owned();

    if rftrace_frontend::dump_full_uftrace(&mut *events, out_dir, &binary_name).is_err() {
        return -1;
    }
    0
}

#[no_mangle]
/// Wraps [`rftrace_frontend::dump_trace`].
pub unsafe extern "C" fn rftrace_dump_trace(events: *mut Events, outfile: *const c_char) -> i64 {
    let Some(outfile) = try_path_from_c_str(CStr::from_ptr(outfile)) else {
        return -1;
    };

    if rftrace_frontend::dump_trace(&mut *events, outfile).is_err() {
        return -1;
    }
    0
}

#[no_mangle]
pub extern "C" fn marker() -> u64 {
    1337
}
