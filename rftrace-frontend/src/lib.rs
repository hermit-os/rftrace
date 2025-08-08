//! This crate provides a possible frontend for rftracer.
//! It can initialize an event buffer, enable/disable tracing and save the trace to disk in a uftrace compatible format.
//! A lot of documentation can be found in the parent workspaces [readme](https://github.com/hermit-os/rftrace).

extern crate byteorder;

mod frontend;
mod interface;

// Re-export frontend functions
pub use frontend::*;
