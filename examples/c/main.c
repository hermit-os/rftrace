#include<stdio.h>
// you can determine header path with `fd -uu .h$ ../../`
#include "../../target/debug/build/rftrace-frontend-ffi-16a32c2014c619e6/out/rftrace_frontend_ffi.h"



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
    rftrace_dump_full_uftrace(events, "tracedir", "test", true);

}