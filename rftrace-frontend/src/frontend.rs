use std::fs::File;
use std::io::prelude::*;
use std::io::{self};

use byteorder::{LittleEndian, WriteBytesExt};

use crate::interface::*;

extern "C" {
    fn rftrace_backend_enable();
    fn rftrace_backend_disable();
    fn rftrace_backend_init(bufptr: *mut Event, len: usize, overwriting: bool);
    fn rftrace_backend_get_events() -> *const Event;
    fn rftrace_backend_get_events_index() -> usize;
}

/// Enables tracing in the backend.
pub fn enable() {
    unsafe { rftrace_backend_enable() }
}

/// Disables tracing in the backend.
pub fn disable() {
    unsafe { rftrace_backend_disable() }
}

/// Used to keep track of event buffer given to the staticlib
#[derive(Copy, Clone, Debug)]
pub struct Events {
    ptr: *mut Event,
    len: usize,
    cap: usize,
}

fn get_events(events: &mut Events) -> (Vec<Event>, usize) {
    // Tell backend to not use the current buffer anymore.
    let ptr = unsafe { rftrace_backend_get_events() };
    println!("{:?}, {:?}", ptr, events);
    assert!(ptr == events.ptr, "Event buffer pointer mismatch!");

    let eventvec = unsafe { Vec::from_raw_parts(events.ptr, events.len, events.cap) };

    let idx = unsafe { rftrace_backend_get_events_index() };
    (eventvec, idx)
}

/// Initializes a new event buffer.
///
/// Allocs a new buffer of size `max_event_count` and passes it to the backend.
/// If `overwriting`, treats it as a ring-buffer, keeping only the most-recent entries, otherwise it stopps logging once it is full.
/// `max_event_count` will not be filled completely, since space is left for the returns of hooked functions.
/// Currently, the maximum stack-depth is 1000. Consequently, `max_event_count` has to be greater than 1000.
pub fn init(max_event_count: usize, overwriting: bool) -> &'static mut Events {
    assert!(
        max_event_count > MAX_STACK_HEIGHT,
        "Event buffer has to be larger than maximum stack height!"
    );
    let buf = vec![Event::Empty; max_event_count];
    unsafe {
        // intentionally leak here! stacks have to live until end of application.
        let (ptr, len, cap) = buf.into_raw_parts();
        rftrace_backend_init(ptr, cap, overwriting);
        // TODO: free this leaked box somewhere. Create a drop() function or similar?
        Box::leak(Box::new(Events { ptr, len, cap }))
    }
}

/// Dumps the traces with some faked metadata into the given folder. Uses the same format as uftrace, which should be used to parse them.
///
/// Will NOT generate symbols! You can generate them with `nm -n $BINARY > binary_name.sym`
///
/// # Arguments
///
/// * `events` - Events buffer to write, returned by `init()`
/// * `out_dir` - folder into which the resulting trace is dumped. Has to exist.
/// * `binary_name` - only relevant for this symbol file. Generated metadata instructs uftrace where to look for it.
///
pub fn dump_full_uftrace(events: &mut Events, out_dir: &str, binary_name: &str) -> io::Result<()> {
    // arbitrary values for pid and sid
    let pid = 42;
    let sid = "00";

    // First lets create all traces.
    let tids = dump_traces(events, out_dir, false)?;

    if tids.is_empty() {
        println!("Trace is empty!");
        return Ok(());
    }

    println!("Creating fake uftrace data dir at {}..", out_dir);
    println!("  Creating ./info");
    let mut info: Vec<u8> = Vec::new();

    // /info HEADER
    // magic
    info.extend("Ftrace!\x00".as_bytes());
    // version. we are using version 4 of fileformat
    info.write_u32::<LittleEndian>(4)
        .expect("Write interrupted");
    // header size. 0x28 == 40 bytes
    info.write_u16::<LittleEndian>(40)
        .expect("Write interrupted");
    // endinaness = 1
    info.push(1);
    // elf_ident[EI_CLASS]. always 2 for 64bit
    info.push(2);
    // feature flags
    println!("    feats = TASK_SESSION | SYM_REL_ADDR");
    const TASK_SESSION: u64 = 1 << 1; // needed.
    const SYM_REL_ADDR: u64 = 1 << 5; // enable symbol relocation (important for ASLR on linux)
    info.write_u64::<LittleEndian>(TASK_SESSION | SYM_REL_ADDR)
        .expect("Write interrupted");
    // info flags
    println!("    info = CMDLINE | TASKINFO");
    const CMDLINE: u64 = 1 << 3; // needed, else --dump chrome outputs invalid json.
    const TASKINFO: u64 = 1 << 7; // needed, since uftrace uses this to determine how to interpret task.txt
    info.write_u64::<LittleEndian>(CMDLINE | TASKINFO)
        .expect("Write interrupted");
    // mstack. disable in feature flags, so 0
    info.write_u16::<LittleEndian>(0)
        .expect("Write interrupted");
    // reserved
    info.write_u16::<LittleEndian>(0)
        .expect("Write interrupted");
    info.write_u16::<LittleEndian>(0)
        .expect("Write interrupted");
    info.write_u16::<LittleEndian>(0)
        .expect("Write interrupted");
    // /info END OF HEADER

    // cmdline
    println!("    cmdline = 'fakeuftrace'");
    writeln!(info, "cmdline:fakeuftrace")?;
    // taskinfo
    println!("    tid = {:?}", tids);
    writeln!(info, "taskinfo:lines=2")?;
    writeln!(info, "taskinfo:nr_tid={}", tids.len())?;
    write!(info, "taskinfo:tids={}", tids[0])?;
    for tid in &tids[1..] {
        write!(info, ",{}", tid)?;
    }
    writeln!(info)?;

    let infofile = format!("{}/info", out_dir);
    let mut infofile = File::create(infofile)?;
    infofile.write_all(&info[..])?;
    drop(infofile);

    println!("  Creating ./task.txt");
    let taskfile = format!("{}/task.txt", out_dir);
    let mut taskfile = File::create(taskfile)?;
    println!("    pid = {}", pid);
    println!("    sid = {}", sid);
    println!("    exe = {}", binary_name);
    writeln!(
        taskfile,
        "SESS timestamp=0.0 pid={} sid={} exename=\"{}\"",
        pid, sid, binary_name
    )?;
    for tid in tids {
        writeln!(taskfile, "TASK timestamp=0.0 tid={} pid={}", tid, pid)?;
    }
    drop(taskfile);

    let mapfilename = format!("{}/sid-{}.map", out_dir, sid);
    let mut mapfile = File::create(mapfilename)?;
    cfg_if::cfg_if! {
        if #[cfg(target_os = "linux")] {
            // see uftrace's record_proc_maps(..)
            // TODO: implement section-merging
            println!(
                "  Creating (incorrect) ./sid-{}.map by copying /proc/self/maps",
                sid
            );
            let mut procfile = File::open("/proc/self/maps")?;
            io::copy(&mut procfile, &mut mapfile)?;
        } else {
            println!("  Creating ./sid-{sid}.map fake memory map file");

            writeln!(mapfile, "000000000000-ffffffffffff r-xp 00000000 00:00 0                          {binary_name}")?;
            writeln!(mapfile, "ffffffffffff-ffffffffffff rw-p 00000000 00:00 0                          [stack]")?;
        }
    }

    if cfg!(target_os = "linux") {
        println!(
            "\nYou should generate symbols with `nm -n $BINARY > {}/$BINARY.sym`",
            out_dir
        );
        println!(
            "INFO: Linux mode is NOT fully supported yet! To get symbols working, you have to"
        );
        println!("      edit the sid-00.map and merge the section for each binary, so that it only occurs once.");
        println!("      Needs to contain at least [stack] and the binaries you want symbols of.");
    } else {
        println!(
            "\nYou should generate symbols with `nm -n $BINARY > {}/{}.sym`",
            out_dir, binary_name
        );
    }

    Ok(())
}

/// Dumps only the trace file to disk, without additional metadata.
///
/// `events` is the Events buffer as returned by `init`.
/// outfile is the file into which the output trace is written.
/// The trace itself has the same format as uftrace, but is not directly parsable due to the missing metadata.
///
/// # Format
/// Packed array of uftrace_record structs
/// ```c
/// struct uftrace_record {
///     uint64_t time;
///     uint64_t type:   2;
///     uint64_t more:   1;
///     uint64_t magic:  3;
///     uint64_t depth:  10;
///     uint64_t addr:   48; /* child ip or uftrace_event_id */
/// };
pub fn dump_trace(events: &mut Events, outfile: &str) -> io::Result<()> {
    dump_traces(events, outfile, true)?;
    Ok(())
}

fn dump_traces(events: &mut Events, outpath: &str, singlefile: bool) -> io::Result<Vec<u64>> {
    // Uftraces trace format: a bunch of 64-bit fields, See https://github.com/namhyung/uftrace/wiki/Data-Format
    //
    // Array of 2x64 bit unsigned long: `[{time: u64, address: u64}, ...]`
    // Since addresses are (currently) only using the low 48 bits, metadata (mainly funciton entry/exit) is saved in the remaining 16 bits.

    /* struct uftrace_record {
        uint64_t time;
        uint64_t type:   2;
        uint64_t more:   1;
        uint64_t magic:  3;
        uint64_t depth:  10;
        uint64_t addr:   48; /* child ip or uftrace_event_id */
    }; */

    // TODO: create enable lock, to ensure no mcount() happens while we read the events array.
    disable();
    println!("Saving traces to disk...!");

    let (events, cidx) = get_events(events);
    let cidx = cidx % events.len();

    // The following is somewhat inefficient, but is intended to solve two constraints:
    // - don't use too much memory. Here we have ~2x trace array.
    // - don't have multiple files open at once

    // To avoid to many reallocs, use array with maximum size for all traces.
    let mut out = Vec::<u8>::with_capacity(16 * events.len());

    // Gather all tids so we can assemble metadata
    let mut tids: Vec<Option<core::num::NonZeroU64>> = Vec::new();
    for e in events[cidx..].iter().chain(events[..cidx].iter()) {
        match e {
            Event::Exit(e) => {
                if !tids.contains(&e.tid) {
                    tids.push(e.tid);
                }
            }
            Event::Entry(e) => {
                if !tids.contains(&e.tid) {
                    tids.push(e.tid);
                }
            }
            Event::Empty => {}
        }
    }

    // For each TID, loop through the events array and save only the relevant items to disk
    for current_tid in &tids {
        // clear out vec in case it contains entries from previous tid
        out.clear();

        let tid = current_tid.map_or(0, |tid| tid.get());

        println!("  Parsing TID {:?}...!", tid);
        for e in events[cidx..].iter().chain(events[..cidx].iter()) {
            match e {
                Event::Exit(e) => {
                    if !singlefile && current_tid != &e.tid {
                        continue;
                    };
                    write_event(&mut out, e.time, e.from, 1);
                }
                Event::Entry(e) => {
                    if !singlefile && current_tid != &e.tid {
                        continue;
                    };
                    write_event(&mut out, e.time, e.to, 0);
                }
                Event::Empty => {
                    continue;
                }
            }
        }

        if !out.is_empty() {
            let filename = if singlefile {
                outpath.into()
            } else {
                let file = format!("{}.dat", tid);
                format!("{}/{}", outpath, file)
            };

            println!(
                "  Writing to disk: {} events, {} bytes ({})",
                out.len() / 16,
                out.len(),
                filename
            );
            let mut file = File::create(filename)?;
            file.write_all(&out[..])?;
        }
    }
    println!("  Parsed all events!");

    // Remove the options from the tids, using 0 for None
    Ok(tids
        .iter()
        .map(|tid| tid.map_or(0, |tid| tid.get()))
        .collect())
}

#[allow(clippy::identity_op)]
#[allow(clippy::erasing_op)]
fn write_event(out: &mut Vec<u8>, time: u64, addr: *const usize, kind: u64) {
    out.write_u64::<LittleEndian>(time)
        .expect("Write interrupted");

    let mut merged: u64 = 0;
    merged |= (kind & 0b11) << 0; // type = UFTRACE_EXIT / UFTRACE_ENTRY
    merged |= 0 << 2; // more, always 0
    merged |= 0b101 << 3; // magic, always 0b101
    merged |= (0 & ((1 << 10) - 1)) << 6; // depth
    merged |= (addr as u64 & ((1 << 48) - 1)) << 16; // actual address, limited to 48 bit.
    out.write_u64::<LittleEndian>(merged)
        .expect("Write interrupted");
}
