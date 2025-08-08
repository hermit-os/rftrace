#include <stddef.h>
#include <stdint.h>

// TODO: merge all definitions
#define MAX_STACK_HEIGHT 1000

// TODO: merge public ABI definitions and use structs here
_Thread_local size_t retstack[1 + MAX_STACK_HEIGHT * 3] = {0};

struct slice {
    void *ptr;
    size_t len;
};

struct slice get_retstack(void) {
    struct slice s = {
        .ptr = &retstack,
        .len = sizeof retstack,
    };

    return s;
}

_Thread_local uint64_t tid = 0;

uint64_t *get_tid(void) {
    return &tid;
}
