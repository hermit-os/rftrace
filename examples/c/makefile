default: out/debug/librftrace.a out/debug/librftrace_frontend_ffi.a
	gcc main.c -p -pthread -ldl -lrftrace -lrftrace_frontend_ffi -Lout/debug/ -o test

out/debug/librftrace.a:
	cargo +nightly build --manifest-path ../../rftrace/Cargo.toml --target-dir out

out/debug/librftrace_frontend_ffi.a:
	cargo +nightly build --manifest-path ../../rftrace-frontend-ffi/Cargo.toml --target-dir out

clean:
	rm -r out
	rm test