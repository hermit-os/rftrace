#!/usr/bin/env python3

import json, struct, shutil, tempfile, subprocess, re, sys, argparse
from pathlib import Path

verbose = True

def log(val):
    if verbose:
        print(val)

UFTRACE_PATH = "uftrace"

def parse_uftrace(uftracedir):
    """ parses uftrace trace to get chrome json file """

    print(f"Converting traces from {uftracedir}")
    uftrace_cmd = [UFTRACE_PATH, 'dump', "--chrome"]

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

    print(f"Parsing kvm traces from {trace_cmd_trace_file}")

    # the full kvmtrace can easily be >1gb file. Even with sed as filtering we get ~4gb output.

    trace_cmd = ['trace-cmd', 'report', '-q', '-i', trace_cmd_trace_file]
    trace = subprocess.Popen(trace_cmd, stdout=subprocess.PIPE)
    
    # align so that we have csv of time,func. Sed is faster and memory friendlier than python here
    #   <...>-105662 [002] 117471684343752: kvm_update_master_clock: masterclock 0 hostclock tsc offsetmatched 0
    #   qemu-system-x86-20667 [010] 15813659115036: kvm_exit:             reason EXIT_IOIO rip 0xec08e info cf80140 ec08f
    sed = ['sed', '-E', r's/.*\[[0-9][0-9][0-9]] ([^ ]*): ([^ ]*):.*/\1,\2/']
    events = subprocess.check_output(sed, stdin = trace.stdout).split(b"\n")
    trace.wait()

    log("events like:")
    log(events[:10])

    # cut off header
    offset = next(i for i,e in enumerate(events) if b"kvm_" in e)
    
    log('events look like:')
    log(events[offset])

    print(f"Parsing {len(events)} KVM events")
    out = []
    in_kvm = True # first event will be a "begin" function event, so we dont get broken frames
    for e in events[offset:]:
        pts = e.strip().split(b",")
        if len(pts) < 2:
            continue
        #name = pts[3][:-1].decode()
        #ts = int(pts[2][:-1])
        ts = int(pts[0]) / 1000.0 
        name = pts[1].decode()
        # make kvm-exit and entry special, so we see the time it is exited. all others get 300ns duration bars
        if name == "kvm_exit" and in_kvm:
            tp = "B" # entry to kvm-host
            name = "kvm exited"
            in_kvm = False
        elif name == "kvm_entry" and not in_kvm:
            tp = "E" # exit from kvm-host
            name = "kvm exited"
            in_kvm = True
        else:
            tp = "X" # generic kvm event.
        
        out.append({
            'pid':77,
            'tid':77,
            'ts':ts,
            'ph':tp, # i = instant event, too small to see.. X = duration event
            'dur':0.3, # gets ignored if we are in entry/exit case
            'name': name
        })

    return out


def get_offset():
    if args.offset == 'auto':
        print("Trying to autodetect offset..")

        # log line where tsc_offset is set
        last_tsc = None

        # if we have a kvm trace, try that for an offset
        if args.kvm:
            # trace-cmd report -q -i tracekvm.dat| grep kvm_write_tsc_offset
            trace_cmd = ['trace-cmd', 'report', '-q', '-i', args.kvm, '-F', 'kvm_write_tsc_offset']
            tscs = subprocess.check_output(trace_cmd).split(b"\n")
            if len(tscs) > 2:
                print("Using kvm trace as offset source")
                log(tscs)
                last_tsc = tscs[-2].decode()
        
        # try directly acessing kernel trace buffer instead
        if not last_tsc:
            try:
                with open('/sys/kernel/debug/tracing/instances/tsc_offset/trace') as f:
                    trace = f.read().split("\n")
                    last_tsc = trace[-2]
            except:
                print("Could not determine offset from kernel tracing. If you intended this, please setup with `setup_kvm_tracing.sh`")
        
        # if any method suceeded in getting tsc log line, parse timestamp
        if last_tsc:
            r = re.search(r"kvm_write_tsc_offset.*next=(\d*)", last_tsc)
            if len(r.groups()) == 0:
                print(f"Cannot parse correct offset from tracing: {last_tsc}!")
                sys.exit(-1)
            offset_raw = int(r.groups()[0])
        else:
            print("Could not determine offset from either source!`")
            offset_raw = 0

        # reinterpret offset u64 as i64
        offset = struct.unpack('q', struct.pack('Q', offset_raw))[0]
    
        print(f"Determined offset as {offset_raw} == {offset}")
    else:
        offset = int(args.offset)

    return offset


def fixup_tids(trace, target_tid):
    # if an event has no tid, set it to target_tid
    for e in trace['traceEvents']:
        if not "tid" in e:
            e['tid'] = target_tid


def fixup_missing_starts(trace, fts, lts):
    # go through trace backwards, keep track of current function stack
    # everytime we pass exit, we push onto stack, on entry we pop again
    # we have separate stacks for each tid/pid pair!

    new_events = []
    stacks = {}
    for e in trace['traceEvents'][::-1]:
        if e['ph'] not in ['E', 'B']:
            # only affects entry and exit events
            continue
        
        if 'tid' not in e:
            print("TID NOT FOUND!")
            print(e)
        
        key = (e['pid'], e['tid'])
        # get stack or set if not exists yet
        stack = stacks.setdefault(key, [])

        if e['ph'] == "E": # exit
            stack.append(e['name'])
        elif e['ph'] == "B": # entry
            if len(stack) > 0:
                func = stack.pop()
                if(e['name'] != func):
                    log(f"Callstack misaligned! expected {func} got")
                    log(e)
            else:
                log("Entered function which we never exited!")
                log(e)
                new_events.append({
                    'pid':e['pid'],
                    'tid':e['tid'],
                    'ts':lts,
                    'ph':"E",
                    'name': e['name']
                })

    for (pid, tid), names in stacks.items():
        for name in names:
            log(f"Exited function which we never entered: {name}!")
            new_events.append({
                'pid':pid,
                'tid':tid,
                'ts':fts,
                'ph':"B",
                'name': name
            })

    log(new_events)
    trace['traceEvents'] += new_events


# parses the counters output by perf with a command like
# sudo perf stat -p $(pgrep -f "qemu-system.*iozone") -I 100 -e cycles:G,kvm:kvm_exit -x\#
# optionally: place "rdtsc 0000000" in the first line to have better offsets!
# returns events, with TS in seconds, not tsc!
def parse_perf_counters(filename, starttime, conversion_fac):
    #  15.127678049#114116639##cycles:G#107341775#100.00##
    #  15.127678049#31961##kvm:kvm_exit#107369492#100.00##

    new_events = []
    with open(filename, "r") as f:
        # try to get better rdtsc, if one is available in counter file.
        head = f.readline()
        if "rdtsc" in head:
            # we have to adjust start time to seconds here, since perf counters already is in seconds since start!
            starttime = int(head.split(" ")[1])/1000 * conversion_fac
            log(f"Updated perf start time from rdtsc-line to {starttime}")
            f.readline() # also skip next line. this is the usual file header
        else:
            print("No accurate timestamp alignment possible for perf counters! They will likely be offset by some tens of ms.")

        for line in f.readlines()[1:]:
            fields = line.strip().split('#')
            if "not counted" in fields[1]:
                # could not sample here? Just continue..
                continue
            time = float(fields[0])
            count = int(fields[1])
            name = fields[3]
            counter_runtime = int(fields[4])
            counter_percentage = float(fields[5]) # time the counter was running if limited available

            # upscale count by counter_percentage
            count = count / (counter_percentage / 100.0)
            new_events.append({
                'pid': 1,
                'ts':starttime + time*1000000, # convert to us
                'ph':"C",
                'name': name,
                'args': {name:count},
            })

    #print(new_events)
    return new_events


def merge():
    # determine the timestamp offset of the guest vm
    offset = get_offset()

    # if binary is specified, generate symbols for trace
    if args.binary:
        print("Generating symbols with nm")
        nm_cmd = ['nm', '-n', args.binary]
        with open(f"{args.TRACE}/{args.binaryname}.sym", "w") as symbolfile:
            subprocess.run(nm_cmd, stdout=symbolfile)
    else:
        print("No binary specified, not generating any symbols!")

    hermit_trace = parse_uftrace(args.TRACE)
    
    if args.merge:
        merge_trace = parse_uftrace(args.merge)
        # uftrace might not include tid data, which will crash the tracy importer.
        # fix up now
        fixup_tids(merge_trace, 1234)
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

        # get first and last timestamp of our trace-cmd trace. Not the best but seems to work
        # needed later to fix some stuff up, like aligning perf stat samples.
        lts = kvm_events[-1]['ts']
        fts = kvm_events[0]['ts']
        log(f"Min/Max ts: {fts} {lts}")
    else:
        lts = None
        fts = None


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
        conversion_fac = 1000000.0/tsc_khz * time_stretch
        print(f"Converting time to ns with tsc_khz={tsc_khz}")
        for event in out["traceEvents"]:
            event['ts'] *= conversion_fac
        if time_stretch != 1:
            print(f"Time is off by a factor of {time_stretch}!")

        # also adapt first/last trace-cmd timestamps
        fts *= conversion_fac
        lts *= conversion_fac

    # add perf traces after time conversion, since they are already in ns
    if perf_kvm_trace:
        if not tsc_khz:
            print("Error: perf is nanosecond aligned -> need to specify tsc_khz for conversion!")
            sys.exit(-1)
        out['traceEvents'] += kvm_events

    # get first timestamp if still missing. will misalign?
    if not fts:
        print("Using hacky first/last timestamp method. This will likely misalign counters?")
        fts = min(out['traceEvents'], key=lambda x: x['ts'] if x['ts'] > 0 else 999999999999999)['ts']
        log(f"Got fts as {fts}")

    # perf stat trace
    if args.perf:
        print(f"Adding perf stat counters")
        # we simply assume that perf started with the first timestamp we have.
        # this is wrong byte the time from perf start to kvm start.
        # but since perf is sampling with ~10hz, this does not matter much.
        # seems to be offset by around 50ms, so just correct for this.
        # this gets overwritten if we provide an 'rdtsc ' line in the perf counter file! 
        kvm_start_offset = 50000
        out['traceEvents'] += parse_perf_counters(args.perf, fts-kvm_start_offset, conversion_fac)

    # filter out unrelevant traces (outside of qemu runtime) if specified.
    if args.filter:
        if not args.kvm:
            print("ERROR: You have to specify a kvm trace to filter! Ignoring option.")
        else:
            out['traceEvents'] = list(filter(lambda e: e['ts'] >= fts and e['ts'] <= lts, out['traceEvents']))
    
    print('Fixing missing start entries if necessary')
    fixup_missing_starts(out, fts, lts)

    print(f"Saving merged trace to {args.OUTPUT}!")
    with open(args.OUTPUT, 'w') as f:
        json.dump(out, f)

def do_uftrace():
    print(f"uftrace mode, ignoring most options!")
    Path(args.OUTPUT).mkdir(parents=True, exist_ok=True)

    create_fake_uftrace(args.OUTPUT, args.TRACE, args.binary)
    print(f"You can view a replay of the trace with `uftrace replay -d {args.OUTPUT}`")


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Merge traces (and/or fix timestamps). Outputs a chrome json trace file.')

    parser.add_argument("TRACE", help="path to one or more guest trace files, as output by the tracing crate")
    parser.add_argument("OUTPUT", help="file or folder where output gets stored")

    parser.add_argument("-b", "--binary", help="path to guest binary, used to generate the symbols of the guest trace")
    parser.add_argument("-B", "--binaryname", help="name of guest binary, has to match fake uftrace metadata", default="test")
    parser.add_argument("-O", "--offset", help="guest <-> host TSC offset. If 'auto', determines it from a) given KVM trace or b) linux tracing", default="auto")
    parser.add_argument("-m", "--merge", help="merge with additional trace, recorded on the host. Has to be a patched-uftrace trace (with TSC time)")
    parser.add_argument("-k", "--kvm", help="path to the trace-cmd trace of kvm samples (trace-cmd record -e 'kvm:*' -C x86-tsc)")
    parser.add_argument("-f", "--freq", help="TSC frequency in khz. This is approx. your cpu frequency. If specified, outputs timestamps into nanoseconds.", type=float)
    parser.add_argument("-F", "--filter", action="store_true", help="filter out all events which happened before kvm started. Need to specify kvm trace!")
    parser.add_argument("-p", "--perf", help="Additional perf stat file for graphs in trace. (perf stat -I 100 -e cycles:G -x\#)")

    args = parser.parse_args()

    # perf KVM Traces have unreliable timestamps, not exposed via argument
    perf_kvm_trace = None

    time_stretch = 1 # can stretch time for better zooming in perfetto ui

    merge()
