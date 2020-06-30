# You need to provide a compiled rusty-loader and virtiofsd.

RUSTY_LOADER = ../../../unmodified/rusty-loader/target/x86_64-unknown-hermit-loader/debug/rusty-loader
BINARY = ../../target/x86_64-unknown-hermit/release/rftrace-hermit-rs-test
VIRTIOFSD = ../../../qemu-master/build/virtiofsd
ROOT_DIR:=$(shell dirname $(realpath $(firstword $(MAKEFILE_LIST))))

default:
	# we need to override the opt level for the dev profile, since we are in a workspace with other settings!
	# not needed if this folder stands on its own, since the profiles in the cargo.toml take effect
	# BUT: we build in release anyway so trace is smaller.
	CARGO_PROFILE_DEV_OPT_LEVEL=1 cargo build -Z build-std=std,core,alloc,panic_abort --target x86_64-unknown-hermit --release

# just gathers the trace from the guest
run: default
	mkdir -p tracedir
	sudo whoami
	sudo $(VIRTIOFSD) --socket-path=/tmp/vhostqemu -o source=$(ROOT_DIR)/tracedir &
	sleep 1
	sudo chmod 777 /tmp/vhostqemu
	qemu-system-x86_64 -cpu qemu64,apic,fsgsbase,rdtscp,xsave,fxsr -enable-kvm -display none -smp 1 -m 1G -serial stdio -kernel $(RUSTY_LOADER) -initrd $(BINARY) -chardev socket,id=char0,path=/tmp/vhostqemu -device vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=tracedir -object memory-backend-file,id=mem,size=1G,mem-path=/dev/shm,share=on -numa node,memdev=mem
	nm -n $(BINARY) > tracedir/test.sym
	uftrace dump -d tracedir --chrome > tracedir/trace.json


# gathers trace from the guest. Merges it with kvm-events gathered from the host.
runkvm: default
	mkdir -p tracedir
	sudo whoami
	# log kvm clock offsets, needed if we wish to merge traces with host
	sudo ../../tools/setup_kvm_tracing.sh

	# setup virtiofsd, so we can write trace to disk later
	sudo $(VIRTIOFSD) --socket-path=/tmp/vhostqemu -o source=$(ROOT_DIR)/tracedir &
	sleep 1
	sudo chmod 777 /tmp/vhostqemu

	# start tracing kvm events
	sudo trace-cmd record -e 'kvm:*' -C x86-tsc -o tracekvm.dat &
	echo "giving trace-cmd time to start"
	sleep 10

	# run qemu
	qemu-system-x86_64 -cpu qemu64,apic,fsgsbase,rdtscp,xsave,fxsr -enable-kvm -display none -smp 1 -m 1G -serial stdio -kernel $(RUSTY_LOADER) -initrd $(BINARY) -chardev socket,id=char0,path=/tmp/vhostqemu -device vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=tracedir -object memory-backend-file,id=mem,size=1G,mem-path=/dev/shm,share=on -numa node,memdev=mem

	# stop tracing kvm
	sudo pkill -2 trace-cmd
	sleep 10

	# merge events, correct time, output json trace
	../../tools/merge.py -b $(BINARY) -f 3593248 -k tracekvm.dat tracedir tracedir/trace.json

clean:
	cargo clean

.PHONY: default clean runkvm run