#!/usr/bin/env python3

import struct, shutil, subprocess, argparse
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


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description='Converts a raw tracefile output by rftrace-frontend to a uftrace with faked metadata. You should probably just dump the full uftrace from the frontend though.')

    parser.add_argument("TRACE", help="path to one or more guest trace files, as output by the tracing crate")
    parser.add_argument("OUTPUT", help="file or folder where output gets stored")
    parser.add_argument("-b", "--binary", help="path to guest binary, used to generate the symbols of the guest trace")
    args = parser.parse_args()

    Path(args.OUTPUT).mkdir(parents=True, exist_ok=True)
    create_fake_uftrace(args.OUTPUT, args.TRACE, args.binary)
    print(f"You can view a replay of the trace with `uftrace replay -d {args.OUTPUT}`")