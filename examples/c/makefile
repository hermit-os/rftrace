default: out/debug/librftrace.so out/debug/librftrace_frontend_ffi.so
	gcc main.c -p -pthread -ldl -lrftrace -lrftrace_frontend_ffi -Lout/debug/ -o test

out/debug/librftrace.so:
	cargo build --manifest-path ../../rftrace/Cargo.toml --target-dir out

out/debug/librftrace_frontend_ffi.so:
	cargo build --manifest-path ../../rftrace-frontend-ffi/Cargo.toml --target-dir out