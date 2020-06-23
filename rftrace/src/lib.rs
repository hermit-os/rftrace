#![feature(naked_functions)]
#![feature(llvm_asm)]
#![feature(thread_local)]
#![feature(linkage)]
#![cfg_attr(feature = "staticlib", no_std)]

mod interface;

#[cfg(feature = "staticlib")]
mod backend;

// Public Interface

// We will link against the 'backend', which is compiled in a separate cargo invocation.
// the backend exports a number of functions we need
// Unfortunately, rust currently does not provide a way to re-export these c-functions from this parent crate without wrapping them!
// Issue: https://github.com/rust-lang/rfcs/issues/2771
// Annoyingly, using rlib these functions get 'silently' exported, so the issue only occurs when we link C code

#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
use crate::interface::*;

// Functions exported by staticlib backend
#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
extern "C" {
    pub fn trs_enable();
    pub fn trs_disable();
    pub fn trs_init(bufptr: *mut Event, len: usize, overwriting: bool);
    pub fn trs_get_events() -> *const Event;
    pub fn trs_get_events_index() -> usize;
}

#[no_mangle]
#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
pub unsafe extern "C" fn rftrace_backend_get_events_index() -> usize {
    trs_get_events_index()
}

#[no_mangle]
#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
pub unsafe extern "C" fn rftrace_backend_get_events() -> *const Event {
    trs_get_events()
}

#[no_mangle]
#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
pub unsafe extern "C" fn rftrace_backend_disable() {
    trs_disable();
}

#[no_mangle]
#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
pub unsafe extern "C" fn rftrace_backend_enable() {
    trs_enable();
}

#[no_mangle]
#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
pub unsafe extern "C" fn rftrace_backend_init(bufptr: *mut Event, len: usize, overwriting: bool) {
    trs_init(bufptr, len, overwriting)
}

#[naked]
#[no_mangle]
#[cfg(all(feature = "reexportsymbols", not(feature = "staticlib")))]
pub unsafe extern "C" fn mcount() {
    llvm_asm!(
        "
    jmp mcount_internal;
    "
    );
}
