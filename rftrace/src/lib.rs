#![feature(naked_functions)]
#![feature(llvm_asm)]
#![feature(thread_local)]
#![feature(linkage)]

#![cfg_attr(feature = "staticlib", no_std)]

mod interface;

#[cfg(feature = "staticlib")]
mod backend;

