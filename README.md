<!-- omit in toc -->
# rftrace - Rust Function Tracer

rftrace is a rust based function tracer. It provides both a backend, which does the actual tracing, and a frontend which write the traces to disk. The backend is designed to standalone and not interact with the system. As such it can be used to partially trace a kernel like [RustyHermit](https://github.com/hermitcore/libhermit-rs) though OS, interrupts, stdlib and application. Multiple threads are supported. It also works for normal Rust and C applications, though better tools exist for that usecase.

Requires a recent nightly rust compiler (as of 28-6-2020).

## Table of Contents
- [Table of Contents](#table-of-contents)
- [Design](#design)
- [Dependencies](#dependencies)
- [Usage](#usage)
  - [Adding rftrace to your application](#adding-rftrace-to-your-application)
    - [Linux Rust application](#linux-rust-application)
    - [RustyHermit](#rustyhermit)
    - [Any other kernel](#any-other-kernel)
  - [Output Format](#output-format)
  - [Chrome trace viewer](#chrome-trace-viewer)
  - [Tracing host applications simultaneously](#tracing-host-applications-simultaneously)
    - [Tracing virtiofsd](#tracing-virtiofsd)
  - [tracing kvm events](#tracing-kvm-events)
  - [Merging traces from different sources](#merging-traces-from-different-sources)
  - [Visualizing the traces](#visualizing-the-traces)
- [Alternative Tracers](#alternative-tracers)
  - [perf](#perf)
  - [uftrace](#uftrace)
  - [Poor Mans Profiler](#poor-mans-profiler)
  - [gsingh93's trace](#gsingh93s-trace)
  - [hawktracer-rust:](#hawktracer-rust)
  - [flamegraph-rs:](#flamegraph-rs)
  - [bpftrace](#bpftrace)
  - [Virtual Machine Introspection](#virtual-machine-introspection)
- [Internals](#internals)
  - [mcount](#mcount)
  - [Time alignment Guest <-> Host](#time-alignment-guest---host)
  - [Build Process](#build-process)
- [Future Work](#future-work)
- [License](#license)
- [Contribution](#contribution)


## Design
I was in need of a function tracer, which works in both kernel and userspace to trace a [RustyHermit](https://github.com/hermitcore/libhermit-rs) application. Preferably without manually annotating source code, as a plug and play solution. Since RustyHermit also has a gcc toolchain, it should work with applications instrumented with both rustc and gcc.

The best way to do this is to use the function instrumentation provided by the compilers, where they insert `mcount()` calls in each function prologue. This is possible in gcc with the `-pg` flag, and in rustc with the newly added  `-Z instrument-mcount` flag. The same mechanism is used with success by eg [uftrace](https://github.com/namhyung/uftrace), which already provides [Rust Support](https://github.com/namhyung/uftrace/issues/594).

This tracer is split into two parts: a backend and a frontend.

The backend is a static library which provides said `mcount()` call and is responsible for logging every function entry and exit into a buffer. It is written in Rust, but is `no_std` and even no alloc. Unlike uftrace, it does not rely on any communication with external software (such as the OS for eg thread-ids). It does require  thread-local-storage though.

Since it is compiled separately as a static library, we can even use a different target architecture. This is needed to easily embed the library into our application, which is for example allowed to use SSE registers. These will cause an abort when used in the wrong situations in the kernel though! By compiling the staticlib against a kernel-target, we avoid this issue and can trace kernel and userspace simultaneously. Another reason for this sub-compilation is, that unlike gcc, rust does not provide a mechanism do selectively disable instrumentation yet. We cannot instrument the `mcount` function itself, else we get infinite recursion.

The frontend interfaces with the backend via a few function calls. It provides the backend with an event-buffer (needed since backend is no-alloc), and is responsible for saving the traces once done. In theory it is easily replacable with your own, but the API is not yet fleshed out.

## Dependencies
The function-prologues of the traced application have to be instrumented with `mcount`. This can be done with the rustc compiler option `-Z instrument-mcount`, or gcc's `-pg` flag.

The backend implicitly assumes a System-V ABI. This affects what registers need to be saved and restored on each function entry and exit, and how funciton-exit-hooking is done. If you use a different convention, check if `mcount()` and `mcount_return_trampoline()` handle the correct registers.

For the logging of callsites and function exits, frame pointers are needed, so make sure your compiler does not omit them as an optimization.

For tracing kernel+application in one trace, a single-address-space OS like HermitCore is needed.
Not all functions can currently be hooked. Naked functions are somewhat broken. Hooking interrupts is broken aswell and will lead to intermittent crashes. Unfortunately, the Rust compiler does have no mechanism to opt-out of `mcount` instrumentation for specific functions, so you have to take care to only enable rftrace in allowed contexts. Currently only runs cleanly if exactly one cpu core is available.

There are no other dependencies required for recording a trace. The output format is the same as the one used by [uftrace](https://github.com/namhyung/uftrace/), so you will need it to view and convert it. There are (currently out-of-date) scripts which can merge traces from multiple different sources in `/tools`, these need `python3`.

When tracing a custom kernel, it needs to provide the capability to write files into a directory, otherwise we cannot save the trace. It also needs to support thread-local-storage, since we use it as a shadow-return-stack and thread-id allocation.

## Usage
There are 4 usage examples in `/examples`: Rust and C, both on normal Linux x64 and RustyHermit. These are the only tested architectures.

### Adding rftrace to your application
#### Linux Rust application

To use rftrace, add both the backend and a frontend to your dependencies.
```toml
[dependencies]
rftrace = "0.1"
rftrace-frontend = "0.1"
```

Ensure that frame pointers are generated! Debug build always seem to have them enabled.

Enable `-Z instrument-mcount`, by either setting environment variable `RUSTFLAGS="-Z instrument-mcount"`, or by including in `.cargo/config`:
```toml
[build]
rustflags=["-Z", "instrument-mcount"]
```

When using vscode, the first can easily be done by modifying your compile task to include
```json
"options": {
    "env": {
        "RUSTFLAGS": "-Z instrument-mcount",
    }
},
```

To actually do the tracing, you have to also add some code to your crate, similar to the following
```rs
fn main() {
    let events = rftrace::init(1000000, true);
    rftrace::enable();

    run_tests();

    rftrace::dump_full_uftrace(events, "/trace", "binaryname", false)
        .expect("Saving trace failed");
}

```

#### RustyHermit
When tracing rusty-hermit, the backend is linked directly to the kernel. This is enabled with the `instrument` feature of hermit-sys (not upstream yet). Therefore we only need the frontend in our application. By using the instrument feature, the kernel is always instrumented. To additionally log functions calls of your application, set the `instrument-mcount` rustflag as seen above.

I further suggest using at least opt-level 2, else a lot of useless clutter will be created by the stdlib. (we are building it ourselves here with `-Z build-std=std,...` so it is affected by the instrument rustflag!)

An example with makefile, which does all the needed trace gathering, timing conversions and kvm-event merging to get a nice trace is provided in `/examples/hermitrust`, and can be compiled and run with `make runkvm`

```toml
[dependencies]
hermit-sys = { path = "../hermit-sys", default-features = false, features = ["instrument"] }
rftrace = "0.1"
```

#### Any other kernel
Unfortunately, there is no way to communicate a fixed, different compilation-target to the backend. There is an open cargo issue for allowing arbitrary environment variables to be set: [Passing environment variables from down-stream to up-stream library](https://github.com/rust-lang/cargo/issues/4121)

For RustyHermit there is a workaround with the `autokernel` feature, which can easily be extended to other targets. Outside of this, you can also set a custom target by setting the environment variable `RFTRACE_TARGET_TRIPLE` to your wanted triple.

Other backend features which might be of interest are:
- `buildcore` - needed for no-std targets. Will build the core library when building the backend.
- `interruptsafe` - enabled by default. Will safe and restore more registers on function exits, to ensure interrupts do not clobber them. Probably only needed when interrupts are instrumented. Can be disabled for performance reasons.


### Output Format
The frontend outputs a trace folder compatible to uftrace: [uftrace's Data Format](https://github.com/namhyung/uftrace/wiki/Data-Format).

Note that the time will be *WRONG*, since we output it in raw TSC counts, and not nanoseconds. You could convert this by determining the TSC frequency and using [merge.py](/tools/merge.py). Also see: [Time alignment Guest <-> Host](#time-alignment-guest---host).

Also note that TID's are not the ones assigned by the host. The backend, having no dependencies at all, does not query TID's, but assigns it's own. The first thread it sees will get TID 1, the second 2..

The full trace consists of 5+ files, 4 for metadata plus 1 per TID which contains the actual trace:
- `/<TID>.dat`: contains trace of thread TID. Might be multiple if multithreaded
- `/info`: general info about cpu, mem, cmdline, version
- `/task.txt`: contains PID, TID, SID<->exename mapping
- `/sid-<SID>.map`: contains mapping of addr to exename. By default, the memory map is faked. You can enable linux-mode, in which case `/proc/self/maps` is copied. 
- `/<exename>.sym`: contains symbols of exe, like output of `nm -n` (has to be sorted!). Symbols are never generated and always have to be done by hand.


### Chrome trace viewer
A very nice way to visualize the trace is using the chrome trace viewer. It can show custom json traces, similar to a flamegraph but interactive. uftrace can convert to this format with `uftrace dump --chrome > trace.json`

- 'Legacy' Interface: open chrome, go to `chrome://tracing`. This opens an interface called [catapult](https://chromium.googlesource.com/catapult/+/HEAD/tracing/README.md).
- 'Modern' Interface: [Perfetto](https://ui.perfetto.dev/#!/viewer). Looks nicer, but has a limited zoom level.
- For both, I suggest using WASD to navigate!
- trace format [documentation](https://docs.google.com/document/d/1CvAClvFfyA5R-PhYUmn5OOQtYMH4h6I0nSsKchNAySU/preview#heading=h.5n45avt6fg8n)


### Tracing host applications simultaneously

One main goal of this tracer was the alignment of traces recorded on the guest and host side. This is possible, since we use the same time source and later align the traces.
I found the easiest way to do so is to [patch uftrace](/patches/uftrace-use-tsc.patch) to use the TSC counter before recording a host-trace.

When having both an officially installed uftrace and a patched one, you have to specify the uftrace-library-path on each invocation:
```sh
uftrace record -L $CUSTOM_PATCHED_UFTRACE APPLICATION
```

#### Tracing virtiofsd
specifically virtiofsd is annoying to trace, since it sandboxes it's process. So no shared memory is possible. Shared memory is referenced by files in `/dev/shm`. Simply mounting `/dev` inside the shared folder did not entierly work, some traces were missing. To get it working, completely disable namespacing with a [patch](/patches/virtiofsd-no-namespacing.patch). This is NOT safe! Guest could potentially modify the whole filesystem if it wants.

We can now run uftrace on virtiofsd as follows:
```sh
sudo uftrace record -L $CUSTOM_PATCHED_UFTRACE ./virtiofsd --thread-pool-size=1 --socket-path=/tmp/vhostqemu -o source=$(pwd)/testdir -o log_level=debug
```

### tracing kvm events
Also nice to have are aligned kvm events. The kernel exposes 73 distinct tracepoints for this. Normally I would use perf for this, but as it turns out it uses it's own unaligned timestamps. It is better to use `trace-cmd`, where we can specify the clock source as TSC.

```sh
sudo perf record -e 'kvm:*' -a sleep 1h
sudo trace-cmd record -e 'kvm:*' -C x86-tsc 
```

### Merging traces from different sources

There is a small [merge.py python script](/tools/merge.py) to merge the traces. See it's help for usage instructions.

An example makefile for gathering events from host+guest, aligning them all and merging them is given in `examples/multi`

### Visualizing the traces
There is an awesome trace recorder and visualizer called [Tracy](https://github.com/wolfpld/tracy). It provides an [importer](https://github.com/wolfpld/tracy/tree/master/import-chrome/src) for the chromium trace format, so traces merged traces can be visualized there. The converter is pretty memory-inefficient though, so it will not work with large traces. In my testing ~15x trace-file-size memory usage, and ~1min processing time for each 100MB chromium-json-trace file.

## Alternative Tracers
A lot of alternatives were considered before writing this crate. Here is a list of options and why they were not chosen.


### perf
[perf](http://www.brendangregg.com/perf.html) is more a sampler than a tracer, though this would still be handy. It even supports tracing KVM guests! Unfortunately only the instruction pointer, and no backtrace is supported. This is only documented in the kernels source though:

Perf uses the kernels event framework. In [arch/x86/events/core.c:perf_callchain_user(..)](https://elixir.bootlin.com/linux/latest/source/arch/x86/events/core.c#L2533) there is a comment: `/* TODO: We don't support guest os callchain now */`.

 For tracing KVM, it uses a function-pointer-struct called [`perf_guest_info_callbacks`](https://elixir.bootlin.com/linux/latest/source/include/linux/perf_event.h#L29), which only contains `is_in_guest`, `is_user_mode`, `get_guest_ip`, `handle_intel_pt_intr`.

The simple case of only recoring ips is done with
`sudo perf kvm --guest record`


### uftrace
[uftrace](https://github.com/namhyung/uftrace/) is a nice all-in-one shop for tracing userspace programs and works great for instrumented native rust binaries.

Since it has lots of features, is contains a lot of code. The most relevant part for us is `libmcount`. This is a library used with `LD_PRELOAD` which provides the `mcount()` call to the instrumented program. Even though libmcount can be build without dependencies, a lot of optional ones are there and used by default. I only need a very small part of it.

Since libmcount is intended for userspace tracing, and I want to embed it into the hermit kernel, a number of issues arise:
- use of shared-memory, 'mounted' via files to communicate the trace results between mcount and uftrace, which is not implemented in RustyHermit
- all parameters get passed via environment variables, which are annoying to set when tracing RustyHermit
- no convenient on/off switch. We cannot trace everything, especially early boot
- written in C -> always need gcc toolchain

Since it's trace format is quite simple, this crate generates a compatible one so we can use uftrace's tooling around it to view and convert it further.


### Poor Mans Profiler
The [Poor Mans Profiler](http://poormansprofiler.org/) is a simple bash script, which calls gdb in a loop. We can use this to pause qemu, which supports gdb well, print a full backtrace, then immediately exit. If we do this often enough, we have a decent albeit slow sampling profiler.

One such invocation takes ~0.15s. The majority of the time is spend in gdb startup though. I have optimized this a bit, and used a gdb-python file to do the same without restarting gdb all the time. This reduces this pause time to ~15ms, a x10 improvement. This is still quite slow though. These `not_quite_as_poor_mans_profiler` scripts can be found in [/tools](/tools).

Unfortunately, virtiofsd contains a race which deadlocks it and qemu when we pause while writing/reading a file. Is is not-trivial to fix it. Bug Report: [virtiofsd deadlocks when qemu is stopped while debugging](https://gitlab.com/virtio-fs/qemu/-/issues/18). Since this is the exact case we want to benchmark, it is unsuitable here.


### gsingh93's trace
[gsingh93's trace](https://github.com/gsingh93/trace) is a nice workaround for needing the unstable `-Z instrument-mcount`: It uses a proc_macro to recurse the Abstract Syntax Tree (AST) and adds tracing calls to every function entry (and potentially exit). It currently calls either `println!()` or `log::trace!()`, but could be easily changed to other calls.

The problem here is that rust does not support 'non-inline modules in proc macros', which means we cannot easily annotate everything in a crate. Most `mod`s would have to be annotated separately, and it would not recurse down into dependencies. This restriction on macros is tested in a [rust test](https://github.com/rust-lang/rust/blob/master/src/test/ui/proc-macro/attributes-on-modules-fail.rs), and tracked in [Tracking issue for procedural macros and "hygiene 2.0"](https://github.com/rust-lang/rust/issues/54727#issuecomment-426409586).

There is an [old implementation](https://github.com/gsingh93/trace/tree/4622ab5d5141d126dd02f66f89f20fd891e73a9d) of this crate which still uses the compiler plugin interface. Problem: The interal AST is quite unstable and the crates code would need to be adapted often to keep up.


### hawktracer-rust:
[hawktracer-rust](https://www.hawktracer.org/) ([GitHub](https://github.com/AlexEne/rust_hawktracer)) provides rust-bindings for Amazons hawktracer. This requires annotating your code with tracepoints, but looks like a nice solution if that is what you want.

### flamegraph-rs: 
[flamegraph-rs](https://github.com/flamegraph-rs/flamegraph) is a simple frontend over perf, and as such not usable for my application. It has a great readme though, which covers lots of tracing background.

### bpftrace
In-kernel VM called eBPF can do a lot of things. It can even be used to perform sampling based profiling:
- [bpftrace](https://github.com/iovisor/bpftrace)
- [Linux Extended BPF (eBPF) Tracing Tools](http://www.brendangregg.com/ebpf.html)
- it pulls the stack traces out of the kernel, just like perf would (in this case via memory-mapped `BPF_STACK_TRACE` and `bpf_get_stack()`)
- we could also use eBPF to walk the stack manually: [Linux eBPF Stack Trace Hack](http://www.brendangregg.com/blog/2016-01-18/ebpf-stack-trace-hack.html)
- as far as I can see, there is no way to interface with KVM from ebpf though!


### Virtual Machine Introspection
There is a set of patches for [kvm-vmi](https://github.com/KVM-VMI/kvm-vmi), which allows for easy debugging, monitoring, analysis and fuzzing of Xen/KVM guests. Unfortunately this is not upstream yet, and needs patches to the kernel and qemu.

There is ongoing effort to merge this into mainline though: [Slides: Leveraging KVM as a Debugging Platform](https://drive.google.com/file/d/1nFoCM62BWKSz2TKhNkrOjVwD8gP51VGK/view)



## Internals

### mcount
Some notes on the mcount() implementation. If you are interested, take a look at the [backend](/src/backend.rs), it is somewhat commented. A few points of interest:

Since `mcount()` calls are only inserted at the beginning of functions, we look at the parents return address, save it on a shadow-stack, and overwrite it with a trampoline. This trampoline will then pop the correct address from the stack and restore it, while also logging that the funciton has exited.

When hooking only rust functions, where LLVM inserts the `mcount()` call, all needed registers are saved before mcount() and restored afterwards. Since it is always inserted right in the beginning of a function, this only affects the functions parameters.

Nontheless, we still backup app potential parameter-registers, just to be on the safe side. There is at least one case, where (admittedly 'incorrect') unsafe Rust code can break otherwise: Use of `asm!` which accesses registers by name instead of relying on LLVM to convert the rust-parameter-name to a register.

It would be quite easy to implement a feature to disable this saving at compile-time, so tracing is faster on supported codebases.

We need to be especially careful when hooking interrupts, since mcount might now get called in the middle of another function and must not clobber any state. The  `interruptsafe` feature is designed to enable this extra safety at a small runtime cost.

For further reading on desiging function tracers, see [Kernel ftrace design](https://www.kernel.org/doc/html/latest/trace/ftrace-design.html). You could also consult uftrace's libmcount.


### Time alignment Guest <-> Host
To align traces taken from the guest and host simultaneously, we need a clock that is very accurate, and constant in both guest and host. x86 cpus include a Time Stamp Counter (TSC). Originally, this counter increased by one every clock cycle. Since this is unwieldy in modern cpu's with changing frequencies and sleep states, two features called `constant_tsc` and `nonstop_tsc` were introduced. With them, the TSC has a fixed frequency, which the linux kernel calls `tsc_khz`. Since OSs are allowed to write into this counter, KVM needs to virtualize it. In the past this was done with kvmclock, and a hypervisor exception when the tsc is written to or read. Modern cpu's have hardware virtualization via offset and scaling 'registers', `tsc_offset`/`tsc_scaling` (you can check if your cpu supports it in `/proc/cpuinfo`).

KVM utilizes these to set the TSC for our guest to zero on boot. This behaviour is non-configurable. Luckliy, we have two ways of determining the exact offset our guest runs at: kernel tracepoints and debugfs.

At `/sys/kernel/debug/kvm/33587-20/vcpu0/tsc-offset` the current tsc-offset of the virtual machine with id `33587-20` can be read. Since this has to be done while the vm is running, and might not catch all changes in this value, we use the second way.

The kernel tracepoint `kvm_write_tsc_offset`, introduced in this [patch](http://lkml.iu.edu/hypermail/linux/kernel/1306.1/01741.html). To register this tracepoint a small script can be used: [setup_kvm_tracing.sh](/tools/setup_kvm_tracing.sh).


Normally, the frequency TSC runs at (`tzc_khz`) is not exposed. You can read it directly from kernel memory by running the following gdb command as root:
```
gdb /dev/null /proc/kcore -ex 'x/uw 0x'$(grep '\<tsc_khz\>' /proc/kallsyms | cut -d' ' -f1) -batch 2>/dev/null | tail -n 1 | cut -f2
```
[StackOverflow: Getting TSC rate in x86 kernel](https://stackoverflow.com/questions/35123379/getting-tsc-rate-in-x86-kernel)

### Build Process
The build process is a bit weird, since we build a static rust library and then link against it.
- cargo build
    - runs `build.rs::build()`
        - compiles part of the code as a native staticlib, by using the Cargo.toml in `/staticlib` and passing the staticlib feature to cargo build
    - compiles the library as normal, without the staticlib feature and links the 'precompiled' static library from the previous step.

We need the second manifest, since it is [not possible to change the library type outside of it](https://github.com/rust-lang/cargo/issues/6160#issuecomment-428778868).

You can compile only static lib manually by renaming `rftrace/staticlib/Cargo.nottoml` to `rftrace/staticlib/Cargo.toml` and running 
`cargo build --manifest-path rftrace/staticlib/Cargo.toml --target-dir target_static --features staticlib -vv`


## Future Work
- create frontend which can output the trace over network, so no file access is needed
- there is a '`no_instrument_function`' LLVM attribute, though not exposed by rust codegen. Might be easy to add an attribute here. See [Make it easy to attach LLVM attributes to Rust functions](https://github.com/rust-lang/rust/issues/15180#issuecomment-137569985). This would remove the need for a staticlib, but only in the case where we are compiling for the same target (not the kernel).
- add option to disable the hooking of returns!
- fix interrupts
- fix multicore behavior


## License

Licensed under either of

 * Apache License, Version 2.0
   ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license
   ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

## Contribution

Unless you explicitly state otherwise, any contribution intentionally submitted
for inclusion in the work by you, as defined in the Apache-2.0 license, shall be
dual licensed as above, without any additional terms or conditions.