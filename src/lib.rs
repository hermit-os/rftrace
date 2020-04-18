#![feature(link_llvm_intrinsics)]
#![feature(naked_functions)]
#![feature(asm)]
#![feature(thread_local)]

//#[macro_use]

//extern crate lazy_static;
#[cfg(feature = "staticlib")]
pub mod trace;

#[cfg(not(feature = "staticlib"))]
extern "C" {
    fn trs_enable();
    fn trs_disable();
    fn trs_print();
}

#[cfg(not(feature = "staticlib"))]
pub fn enable() {
    unsafe{trs_enable()}
}
#[cfg(not(feature = "staticlib"))]
pub fn disable() {
    unsafe{trs_disable()}
}
#[cfg(not(feature = "staticlib"))]
pub fn print() {
    unsafe{trs_print()}
}


#[cfg(feature = "staticlib")]
#[no_mangle]
#[cfg(feature = "staticlib")]
pub fn trs_enable() {
    trace::enable();
}

#[cfg(feature = "staticlib")]
#[no_mangle]
#[cfg(feature = "staticlib")]
pub extern "C" fn trs_disable() {
    trace::disable();
}

#[cfg(feature = "staticlib")]
#[no_mangle]
#[cfg(feature = "staticlib")]
pub extern "C" fn trs_print() {
    trace::print();
}