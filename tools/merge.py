#!/usr/bin/env python3

import json, struct, shutil, tempfile, subprocess, re, sys, argparse
from pathlib import Path

def create_fake_uftrace(dirname, tracefile, binary=None, PID=123, TID=42, SID=b"00"):
    """ Creates a fake uftrace from just a trace.dat file + the original binary for symbols.
    other params can be chosen freely. Not really important, just cosmetics
    """

    print(f"Creating fake uftrace data dir at {dirname}..")
    print("  Creating /info")
    print("    feats = TASK_SESSION")
    TASK_SESSION = 1 << 1 # needed.
    feats = struct.pack("<Q", TASK_SESSION)

    print("    info = CMDLINE | TASKINFO")
    CMDLINE = 1 << 3 # needed, else --dump chrome outputs invalid json.
    TASKINFO = 1 << 7 # needed, since uftrace uses this to determine how to interpret task.txt
    infos = struct.pack("<Q", CMDLINE | TASKINFO)
    
    print(f"    cmdline = 'fakeuftrace'")
    print(f"    tid = {TID}")

    rest  = b"cmdline:fakeuftrace\n"
    rest += b"taskinfo:lines=2\n"
    rest += b"taskinfo:nr_tid=1\n"
    rest += b"taskinfo:tids=%d\n" % TID

    magic = b"Ftrace!\x00"
    version = b"\x04\x00\x00\x00" # we are using version 4 of fileformat
    size = b"\x28\x00" # 0x28 == 40 bytes
    endian = b"\x01"
    classs = b"\x02" # elf_ident[EI_CLASS]. always 2 for 64bit
    mstack = b"\x00\x00" # disabled feature
    reserved = b"\x00"*6 # reverved, always 0

    with open(f"{dirname}/info", "wb") as f:
        f.write(magic+version+size+endian+classs+feats+infos+mstack+reserved+rest)

    if binary:
        EXENAME = binary.split("/")[-1]
    else:
        EXENAME = "tracedguest"

    print("  Creating /task.txt")
    print(f"    pid = {PID}")
    print(f"    sid = {SID.decode()}")
    print(f"    exe = {EXENAME}")
    tasktxt  = b"SESS timestamp=0.0 pid=%d sid=%s exename=\"%s\"\n" % (PID, SID, EXENAME.encode())
    tasktxt += b"TASK timestamp=0.0 tid=%d pid=%d\n" % (TID, PID)

    with open(f"{dirname}/task.txt", "wb") as f:
        f.write(tasktxt)

    print(f"  Creating /sid-{SID.decode()}.map memory map file")
    memmap  = b"000000000000-7f0000000000 r-xp 00000000 00:00 0                          %s\n" % EXENAME.encode()
    memmap += b"7f0000000000-7fffffffffff rw-p 00000000 00:00 0                          [stack]\n"

    with open(f"{dirname}/sid-{SID.decode()}.map", "wb") as f:
        f.write(memmap)

    # copy trace data
    print("  Copying trace file")
    shutil.copyfile( tracefile , f"{dirname}/{TID}.dat" )

    # generate symbols
    if binary:
        print("  Generating symbols with nm")
        nm_cmd = ['nm', '-n', binary]
        with open(f"{dirname}/{EXENAME}.sym", "w") as symbolfile:
            subprocess.run(nm_cmd, stdout=symbolfile)
    else:
        print("  No binary specified, not generating any symbols!")

    print("Done!")


def parse_uftrace(uftracedir):
    """ parses uftrace trace to get chrome json file """

    print(f"Converting traces from {uftracedir}")
    uftrace_cmd = ['uftrace', 'dump', "--chrome"]

    js = subprocess.check_output(uftrace_cmd, cwd=uftracedir)
    trace = json.loads(js)

    return trace


def parse_perf_trace(perf_trace_file):
    """ parses a perf-recording of kvm-events

    perf CLOCK IS MISALIGNED. use trace-cmd instead!

    # enable tracing access for ALL users (else perf needs sudo)
    sudo sysctl kernel.perf_event_paranoid=-1
    # record all kvm events
    perf record -e 'kvm:*' -a sleep 1h
    perf script -F trace:time,event --ns
    """

    perf_cmd = ['perf', 'script', '-F', 'trace:time,event', '--ns', '-i', perf_trace_file]
    events = subprocess.check_output(perf_cmd).split(b"\n")
    print(f"Parsing {len(events)} KVM events")
    out = [{'pid':1, 'ts':int(e[:15].replace(b".",b""))/1000, 'ph':'i','name':e[16:-2].strip().decode()} for e in events if e]
    return out


def parse_tracecmd_trace(trace_cmd_trace_file):
    """ parses a trace-cmd recording of kvm-events

    sudo trace-cmd record -e 'kvm:*' -C x86-tsc
    """

    trace_cmd = ['trace-cmd', 'report', '-q', '-i', trace_cmd_trace_file]
    events = subprocess.check_output(trace_cmd).split(b"\n")

    # cut off header
    offset = next(i for i,e in enumerate(events) if b"<...>" in e)
    
    #   <...>-105662 [002] 117471684343752: kvm_update_master_clock: masterclock 0 hostclock tsc offsetmatched 0
    def parse(e):
        pts = e.strip().split(b" ")
        name = pts[3][:-1].decode()
        # make kvm-exit and entry special, so we see the time it is exited. all others get 300ns duration bars
        if name == "kvm_exit":
            tp = "B" # entry to kvm-host
            name = "kvm exited"
        elif name == "kvm_entry":
            tp = "E" # exit from kvm-host
            name = "kvm exited"
        else:
            tp = "X" # generic kvm event.
        return {
            'pid':1,
            'tid':1,
            'ts':int(pts[2][:-1])/1000.0,
            'ph':tp, # i = instant event, too small to see.. X = duration event
            'dur':0.3, # gets ignored if we are in entry/exit case
            'name': name
        }

    print(f"Parsing {len(events)} KVM events")
    out = [parse(e) for e in events[offset:] if e]
    return out



def do_chrome():
    if args.offset == 'auto':
        print("Trying to autodetect offset..")

        print("TODO: check kvm trace!")

        try:
            with open('/sys/kernel/debug/tracing/instances/tsc_offset/trace') as f:
                trace = f.read().split("\n")
                last = trace[-2]
                r = re.search(r"kvm_write_tsc_offset.*next=(\d*)", last)
                if len(r.groups()) == 0:
                    print(f"Cannot parse correct offset from tracing: {last}!")
                    sys.exit(-1)
                offset_raw = int(r.groups()[0])
        except:
            print("Could not determine offset from kernel tracing. If you intended this, please setup with `setup_kvm_tracing.sh`")
            offset_raw = 0
        
        # reinterpret offset u64 as i64
        offset = struct.unpack('q', struct.pack('Q', offset_raw))[0]
    
        print(f"Determined offset as {offset_raw} == {offset}")
    else:
        offset = int(args.offset)

    # use uftrace to convert traces to chrome json trace format
    with tempfile.TemporaryDirectory() as tmpdirname:
        print('Created temporary directory', tmpdirname)
        create_fake_uftrace(tmpdirname, args.TRACE, args.binary)

        hermit_trace = parse_uftrace(tmpdirname)
    
    if args.merge:
        merge_trace = parse_uftrace(args.merge)
    else:
        merge_trace = None

    if offset:
        print(f"Offseting guest traces by {offset} counts")
        
        for event in hermit_trace["traceEvents"]:
            if event['ts'] > 0:
                event['ts'] -= offset/1000
            else:
                print("Not offsetting:", event)

    # perf/kvm traces
    if perf_kvm_trace:
        kvm_events = parse_perf_trace(perf_kvm_trace)
    if args.kvm:
        kvm_events = parse_tracecmd_trace(args.kvm)

    # merging
    print("Merging traces")
    out = hermit_trace

    if merge_trace:
        out['traceEvents'] += merge_trace['traceEvents']
    if args.kvm:
        out['traceEvents'] += kvm_events

    # TSC -> ns conversion
    tsc_khz = args.freq
    if tsc_khz:
        print(f"Converting time to ns with tsc_khz={tsc_khz}")
        for event in out["traceEvents"]:
            event['ts'] *= 1000000.0/tsc_khz * time_stretch
        if time_stretch != 1:
            print(f"Time is off by a factor of {time_stretch}!")

    # add perf traces after time conversion, since they are already in ns
    if perf_kvm_trace:
        if not tsc_khz:
            print("Error: perf is nanosecond aligned -> need to specify tsc_khz for conversion!")
            sys.exit(-1)
        out['traceEvents'] += kvm_events

    if args.filter:
        if not args.kvm:
            print("ERROR: You have to specify a kvm trace to filter! Ignoring option.")
        else:
            kvmstart = kvm_events[0]['ts']
            out['traceEvents'] = list(filter(lambda e: e['ts'] >= kvmstart, out['traceEvents']))

    print(f"Saving merged trace to {args.OUTPUT}!")
    with open(args.OUTPUT, 'w') as f:
        json.dump(out, f)


def do_uftrace():
    print(f"uftrace mode, ignoring most options!")
    Path(args.OUTPUT).mkdir(parents=True, exist_ok=True)

    create_fake_uftrace(args.OUTPUT, args.TRACE, args.binary)
    print(f"You can view a replay of the trace with `uftrace replay -d {args.OUTPUT}`")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Convert and merge traces. Can output either a chrome json trace file, or converted pseudo uftrace.')

    parser.add_argument("TRACE", help="path to one or more guest trace files, as output by the tracing crate")
    parser.add_argument("OUTPUT", help="file or folder where output gets stored")

    parser.add_argument("--mode", help="""
                                output mode, defaults to 'chrome'.
                                'chrome': Convert and merge traces, output single json file.
                                'uftrace': Convert to uftrace, outputs trace directory.
                                """, default="chrome", choices=["chrome", "uftrace"])

    parser.add_argument("-b", "--binary", help="path to guest binary, used to generate the symbols of the guest trace")
    parser.add_argument("-O", "--offset", help="guest <-> host TSC offset. If 'auto', determines it from a) given KVM trace or b) linux tracing", default="auto")
    parser.add_argument("-m", "--merge", help="merge with additional trace, recorded on the host. Has to be a patched-uftrace trace (with TSC time)")
    parser.add_argument("-k", "--kvm", help="path to the trace-cmd trace of kvm samples (trace-cmd record -e 'kvm:*' -C x86-tsc)")
    parser.add_argument("-f", "--freq", help="TSC frequency in khz. This is approx. your cpu frequency. If specified, outputs timestamps into nanoseconds.", type=float)
    parser.add_argument("-F", "--filter", action="store_true", help="filter out all events which happened before kvm started. Need to specify kvm trace!")

    args = parser.parse_args()

    # perf KVM Traces have unreliable timestamps, not exposed via argument
    perf_kvm_trace = None

    time_stretch = 1 # can stretch time for better zooming in perfetto ui

    if args.mode == "chrome":
        do_chrome()
    else:
        do_uftrace()
