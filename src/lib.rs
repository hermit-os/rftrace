#![feature(naked_functions)]
#![feature(llvm_asm)]
#![feature(thread_local)]
#![feature(linkage)]

#![cfg_attr(feature = "frontend", feature(vec_into_raw_parts))]
#![cfg_attr(feature = "staticlib", no_std)]

#[cfg(feature = "frontend")]
extern crate byteorder;

mod interface;

#[cfg(feature = "staticlib")]
mod backend;

#[cfg(feature = "frontend")]
mod frontend;


// Re-export frontend functions
#[cfg(feature = "frontend")]
pub use frontend::*;