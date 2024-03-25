#include <stdio.h>

#include "../../rftrace-frontend-ffi/rftrace_frontend_ffi.h"

void func3() {
    printf("Func3!\n");
}

void func2() {
    printf("Func2!\n");
    func3();
}

void func1() {
    printf("Func1!\n");
    func2();
}

void main() {
    printf("Starting tracing...\n");
    Events *events = rftrace_init(10000, false);
    rftrace_enable();
    func1();
    func1();
    func1();
    rftrace_dump_full_uftrace(events, "/tracedir", "example");
}

/// Just a few dummy functions if smoltcp support is disabled
void sys_tcp_stream_connect() {}
void sys_tcp_stream_read() {}
void sys_tcp_stream_write() {}
void sys_tcp_stream_close() {}
void sys_tcp_stream_shutdown() {}
void sys_tcp_stream_set_read_timeout() {}
void sys_tcp_stream_get_read_timeout() {}
void sys_tcp_stream_set_write_timeout() {}
void sys_tcp_stream_get_write_timeout() {}
void sys_tcp_stream_duplicate() {}
void sys_tcp_stream_peek() {}
void sys_tcp_stream_set_nonblocking() {}
void sys_tcp_stream_set_tll() {}
void sys_tcp_stream_get_tll() {}
void sys_tcp_stream_peer_addr() {}
void sys_tcp_listener_accept() {}
void sys_network_init() {}
void init_lwip() {}
void lwip_read() {}
void lwip_write() {}
