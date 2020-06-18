use core::arch::x86_64::_rdtsc;
use core::slice;
use core::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

use crate::interface::*;

#[derive(Clone, Copy)]
struct RetStack {
    pub stack: [SavedRet; MAX_STACK_HEIGHT],
    pub index: usize,
}

#[derive(Debug, Clone, Copy)]
struct SavedRet {
    pub stackloc: *mut *const usize,
    pub retloc: *const usize,
    pub childip: *const usize,
}

static mut ENABLED: bool = false;
static mut OVERWRITING: bool = false; // should the ring-buffer be overwritten once full?
static mut INDEX: AtomicUsize = AtomicUsize::new(0);
static mut EVENTS: Option<&mut [Event]> = None;

// !! Will always be initialized to all 0 by the OS, no matter what. This is just to make the compiler happy
#[thread_local]
static mut RETSTACK: RetStack = RetStack {
    stack: [SavedRet {
        stackloc: 0 as *mut *const usize,
        retloc: 0 as *const usize,
        childip: 0 as *const usize,
    }; MAX_STACK_HEIGHT],
    index: 0,
};

#[thread_local]
static mut TID: Option<core::num::NonZeroU64> = None;

// Everytime we see a new thread (with emtpy thread-locals), we alloc out own TID
static mut TID_NEXT: AtomicU64 = AtomicU64::new(1);

// Need to define own panic handler, since we are no_std
use core::panic::PanicInfo;
#[linkage = "weak"]
#[panic_handler]
fn panic(_info: &PanicInfo) -> ! {
    loop {}
}

impl RetStack {
    /*pub fn new(capacity: usize) -> RetStack {
        //println!("Creating retstack...!");
        RetStack{vec: RefCell::new(Vec::with_capacity(capacity)), capacity}
    }*/

    pub fn push(&mut self, item: SavedRet) -> Result<(), ()> {
        if self.index >= self.stack.len() {
            // Stack full!
            return Err(());
        }

        self.stack[self.index] = item;
        self.index += 1;
        Ok(())
    }

    pub fn pop(&mut self) -> Option<SavedRet> {
        if self.index == 0 {
            return None;
        }
        self.index -= 1;
        Some(self.stack[self.index])
    }
}

#[naked]
#[no_mangle]
pub extern "C" fn mcount() {
    // We need to be careful with hooked naked functions!
    // Normally, llvm ensures that all needed functions parameters are saved before the embedded mcount() is called, and restored afterwards.
    // This does NOT happen with naked funktions like `hermit::arch::x86_64::kernel::switch::switch:`
    // There, the args (like old and new_stack) are clobbered. This is because they are not used, only implicitly in the llvm_asm! code, so llvm does not know they are used!
    // To be sure the instrumentation never breaks anything, we backup and restore any possible argument registers
    // TODO: Implement feature to skip this, which can be enabled if we are sure this can't happen with the code we are instrumenting?

    // we need custom assembly that "knows" that mcount is ALWAYS called at the start of each function! no llvm magic can help here.
    // parents-return-addr is always stored at rbp+8
    // mcounts ret addr is directly at rsp

    // based on https://github.com/namhyung/uftrace/blob/master/arch/x86_64/mcount.S
    unsafe {
        if !ENABLED {
            return;
        }
        llvm_asm!("
        /* make some space for locals on the stack */
        sub $$48, %rsp

        /* save register arguments in mcount_args. Needed so we can later restore them */
        movq %rdi, 40(%rsp)
        movq %rsi, 32(%rsp)
        movq %rdx, 24(%rsp)
        movq %rcx, 16(%rsp)
        movq %r8,   8(%rsp)
        movq %r9,   0(%rsp)

        /* child addr = what function was mcount() called from */
        movq 48(%rsp), %rsi

        /* parent location = child-return-addr-ptr = what addr stores the location the child function was called from */
        /* needed, since we overwrite it with our own trampoline. This way we can determine when the child function returns */
        lea 8(%rbp), %rdi


        /* align stack pointer to 16-byte, remember old value */
        movq %rsp, %rdx
        andq $$0xfffffffffffffff0, %rsp

        /* pass mcount_args to mcount_entry's 3rd argument */
        push %rdx

        /* save rax (implicit argument for variadic functions) */
        push %rax

        call mcount_entry

        /* restore rax */
        pop  %rax

        /* restore original stack pointer */
        pop  %rdx
        movq %rdx, %rsp

        /* restore mcount_args */
        movq  0(%rsp), %r9
        movq  8(%rsp), %r8
        movq 16(%rsp), %rcx
        movq 24(%rsp), %rdx
        movq 32(%rsp), %rsi
        movq 40(%rsp), %rdi

        /* revert stack pointer to original location and return */
        add $$48, %rsp
        retq
        ");
    }
}

#[no_mangle]
pub extern "C" fn mcount_entry(parent_ret: *mut *const usize, child_ret: *const usize) {
    unsafe {
        if ENABLED {
            // HermitCore's task creation will set rbp to 0 in the first function for the task: task_entry()
            // This means parent_ret (which is lea 8(%rbp)), will be 8 and we will crash on access.
            // Other OS's likely do something similar. So we never do anything if we do not have our own TID yet,
            // Which always happens on the first call. From the second onwards we log calls.
            match TID {
                None => {
                    // We are not yet initialized, do it now
                    // Would only fail if we overflow TID_NEXT, which is 64bit, then TID stays None (?)
                    TID = core::num::NonZeroU64::new(TID_NEXT.fetch_add(1, Ordering::Relaxed));
                    return;
                }
                Some(tid) => tid,
            };

            //if parent_ret as usize == 8 {
            //    return;
            //}

            // Save call to global events ringbuffer
            if let Some(events) = &mut EVENTS {
                // Get current globally-unique-event-index
                let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
                if !OVERWRITING && cidx >= events.len() - MAX_STACK_HEIGHT {
                    disable();
                    return;
                }

                events[cidx % events.len()] = Event::Entry(Call {
                    time: _rdtsc(),
                    to: child_ret,
                    from: *parent_ret,
                    tid: TID.as_ref().copied(),
                });
            }

            let sr = SavedRet {
                stackloc: parent_ret,
                retloc: *parent_ret,
                childip: child_ret,
            };
            // Do not overwrite ret-ptr if returnstack is full
            // this will lead to truncation of the return events once a too big stack has been reached!
            // TODO: warn the user about this?
            if RETSTACK.push(sr).is_ok() {
                *parent_ret = mcount_return_trampoline as *const usize;
            }
        }
    }
}

#[naked]
pub extern "C" fn mcount_return_trampoline() {
    // does 'nothing', except calling mcount_return. Takes care to not clobber any return registers.
    // based on https://github.com/namhyung/uftrace/blob/master/arch/x86_64/mcount.S

    unsafe {
        /* save registers which could contain return values */
        llvm_asm!("
            /* space for locals (saved ret values) (if we dont back up xmm0+1, this is too much, but this won't hurt us) */
            sub $$64, %rsp

            movq   %rdx,   8(%rsp)
            movq   %rax,   0(%rsp)
        ");

        /* when we compile against a 'kernel' target we do NOT have sse enabled, otherwise we might. Backup xmm0 and xmm1 in that case */
        #[cfg(target_feature = "sse2")]
        llvm_asm!(
            "
            movdqu %xmm0, 16(%rsp)
            movdqu %xmm1, 32(%rsp)
        "
        );

        llvm_asm!("
            /* set the first argument of mcount_return as pointer to return values */
            movq %rsp, %rdi

            /* call mcount_return, which returns original parent address. Store it at the correct stack location */
            call mcount_return
            movq %rax, 56(%rsp)

            /* restore saved return values */
            movq    0(%rsp), %rax
            movq    8(%rsp), %rdx
        ");

        /* Restore sse return values, if supported */
        #[cfg(target_feature = "sse2")]
        llvm_asm!(
            "
            movdqu 16(%rsp), %xmm0
            movdqu 32(%rsp), %xmm1
        "
        );

        llvm_asm!(
            "
            /* add only 56 to rsp, so the missing 8 become the new return pointer */
            add $$56, %rsp
            retq
        "
        );
    }
}

#[no_mangle]
pub extern "C" fn mcount_return() -> *const usize {
    unsafe {
        let (original_ret, childip) = {
            let sr = RETSTACK.pop().expect("retstack empty?");

            (sr.retloc, sr.childip)
        };

        let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
        if let Some(events) = &mut EVENTS {
            events[cidx % events.len()] = Event::Exit(Exit {
                time: _rdtsc(),
                from: childip,
                tid: TID.as_ref().copied(),
            });
        }

        original_ret
    }
}

fn disable() {
    unsafe {
        ENABLED = false;
    }
}

fn enable() {
    //println!("enabling mcount hooks..");
    unsafe {
        ENABLED = true;
    }
}

fn set_eventbuf(eventbuf: &'static mut [Event]) {
    unsafe {
        if EVENTS.is_some() {
            // ERROR! already initialized
            return;
        }

        EVENTS.replace(eventbuf);
    }
}

// Public interface

#[no_mangle]
pub extern "C" fn trs_get_events_index() -> usize {
    return unsafe { INDEX.load(Ordering::Relaxed) };
}

#[no_mangle]
pub extern "C" fn trs_get_events() -> *const Event {
    return unsafe {
        EVENTS
            .take()
            .map(|e| e.as_ptr())
            .unwrap_or(0 as *const Event)
    };
}

#[no_mangle]
pub extern "C" fn trs_disable() {
    disable();
}

#[no_mangle]
pub fn trs_enable() {
    enable();
}

#[no_mangle]
pub extern "C" fn trs_init(bufptr: *mut Event, len: usize, overwriting: bool) {
    let eventbuf = unsafe {
        assert!(!bufptr.is_null());
        slice::from_raw_parts_mut(bufptr, len)
    };

    assert!(
        len > MAX_STACK_HEIGHT,
        "Event buffer has to be larger than maximum stack height!"
    );

    unsafe {
        OVERWRITING = overwriting;
    }

    set_eventbuf(eventbuf);
}