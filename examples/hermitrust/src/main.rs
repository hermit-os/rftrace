extern crate rftrace as _;
use rftrace_frontend as rftrace;

#[cfg(target_os = "hermit")]
extern crate hermit_sys;

use std::thread;

fn main() {
    let events = rftrace::init(100000, false);
    rftrace::enable();
    println!("Hello, world!");
    test1();
    rftrace::dump_full_uftrace(events, "/tracedir", "test", false).expect("");
}

fn test1() {
    println!("test1");
    test2();
}

fn test2() {
    println!("test2");
    test3();
}

fn test3() {
    println!("test3");
    threads();
}

fn threads() {
    let mut children = vec![];

    for i in 0..4 {
        // Spin up another thread
        children.push(thread::spawn(move || {
            println!("this is thread number {}", i);
        }));
    }

    for child in children {
        // Wait for the thread to finish. Returns a result.
        let _ = child.join();
    }
}