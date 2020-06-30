# You need to provide a compiled rusty-loader, hermit-gcc, the libhermit-rs source and virtiofsd.

TARGET = x86_64-unknown-hermit

GCC = ../../../toolchain/bin/x86_64-hermit-gcc
RUSTY_LOADER = ../../../unmodified/rusty-loader/target/x86_64-unknown-hermit-loader/debug/rusty-loader
VIRTIOFSD = ../../../qemu-master/build/virtiofsd
LIBHERMIT_SRC = ../../../libhermit-rs

LIBRARY_PATH = out/$(TARGET)/debug/
HERMIT_PATH = out/$(TARGET)-kernel/release/

ROOT_DIR:=$(shell dirname $(realpath $(firstword $(MAKEFILE_LIST))))

RUST_FLAGS = "-Z instrument-mcount"


default: example

example: main.c $(LIBRARY_PATH)/librftrace.a $(LIBRARY_PATH)/librftrace_frontend_ffi.a $(HERMIT_PATH)/libhermit.a
	$(GCC) main.c -p -static -lrftrace_frontend_ffi -lrftrace -lhermit -L$(LIBRARY_PATH) -L$(HERMIT_PATH) -o example

$(HERMIT_PATH)/libhermit.a:
	RUSTFLAGS=$(RUST_FLAGS) cargo build --manifest-path $(LIBHERMIT_SRC)/Cargo.toml -Z build-std=core,alloc --target $(TARGET)-kernel --features newlib  --release --target-dir out

$(LIBRARY_PATH)/librftrace.a:
	cargo build --manifest-path ../../rftrace/Cargo.toml --target $(TARGET) --features autokernel,buildcore --target-dir out -Z build-std=std,panic_abort

$(LIBRARY_PATH)/librftrace_frontend_ffi.a:
	RUSTFLAGS=$(RUST_FLAGS) cargo build --manifest-path ../../rftrace-frontend-ffi/Cargo.toml --target $(TARGET) --target-dir out -Z build-std=std,panic_abort
	# we need to strip runtime_entry, since it is defined by newlibc AND std. This should probably be solved differently.
	objcopy --strip-symbol runtime_entry $@

hermit: $(HERMIT_PATH)/libhermit.a

clean:
	rm -r out
	[ -f example ] && rm example

run: example
	mkdir -p tracedir
	sudo whoami
	sudo $(VIRTIOFSD) --socket-path=/tmp/vhostqemu -o source=$(ROOT_DIR)/tracedir &
	sleep 1
	sudo chmod 777 /tmp/vhostqemu
	qemu-system-x86_64 -cpu qemu64,apic,fsgsbase,rdtscp,xsave,fxsr -enable-kvm -display none -smp 1 -m 1G -serial stdio -kernel $(RUSTY_LOADER) -initrd $(ROOT_DIR)/example -chardev socket,id=char0,path=/tmp/vhostqemu -device vhost-user-fs-pci,queue-size=1024,chardev=char0,tag=tracedir -object memory-backend-file,id=mem,size=1G,mem-path=/dev/shm,share=on -numa node,memdev=mem
	nm -n $(ROOT_DIR)/example > tracedir/example.sym
	uftrace dump -d tracedir --chrome > tracedir/trace.json

.PHONY: default hermit clean run