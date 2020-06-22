#![feature(vec_into_raw_parts)]

extern crate byteorder;

mod frontend;
mod interface;

// Re-export frontend functions
pub use frontend::*;
