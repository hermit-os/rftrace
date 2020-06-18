#![feature(vec_into_raw_parts)]

extern crate byteorder;

mod interface;
mod frontend;

// Re-export frontend functions
pub use frontend::*;
