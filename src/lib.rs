#![feature(naked_functions)]
#![feature(asm)]
#![feature(thread_local)]
#![feature(linkage)]

#![cfg_attr(feature = "frontend", feature(vec_into_raw_parts))]
#![cfg_attr(feature = "staticlib", no_std)]

#[cfg(feature = "frontend")]
extern crate byteorder;

pub mod interface;

#[cfg(feature = "staticlib")]
pub mod backend;

#[cfg(feature = "frontend")]
pub mod frontend;
