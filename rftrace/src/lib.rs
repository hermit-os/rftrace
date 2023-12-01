//! Backend for rftrace.
//! Provides an `mcount` implementation, which does nothing by default but can be enabled via frontend.
//! A lot of documentation can be found in the parent workspaces [readme](https://github.com/tlambertz/rftrace).

#![cfg_attr(feature = "staticlib", feature(asm))]
#![cfg_attr(feature = "staticlib", feature(naked_functions))]
#![cfg_attr(feature = "staticlib", feature(llvm_asm))]
#![cfg_attr(feature = "staticlib", feature(thread_local))]
#![cfg_attr(feature = "staticlib", no_std)]

mod interface;

#[cfg(feature = "staticlib")]
mod backend;
