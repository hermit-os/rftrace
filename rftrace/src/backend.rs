use core::arch::x86_64::_rdtsc;
use core::slice;
use core::sync::atomic::{AtomicBool, AtomicU64, AtomicUsize, Ordering};

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

static ENABLED: AtomicBool = AtomicBool::new(false);
static OVERWRITING: AtomicBool = AtomicBool::new(false); // should the ring-buffer be overwritten once full?
static INDEX: AtomicUsize = AtomicUsize::new(0);
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
        if !ENABLED.load(Ordering::Relaxed) {
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
        if ENABLED.load(Ordering::Relaxed) {
            let tid = match TID {
                None => {
                    // We are not yet initialized, do it now
                    // Would only fail if we overflow TID_NEXT, which is 64bit, then TID stays None (?)
                    TID = core::num::NonZeroU64::new(TID_NEXT.fetch_add(1, Ordering::Relaxed));
                    TID
                }
                Some(tid) => Some(tid),
            };

            // HermitCore's task creation will set rbp to 0 in the first function for the task: task_entry()
            // This means parent_ret (which is lea 8(%rbp)), will be 8 and we will crash on access.
            // Other OS's likely do something similar. Don't deref in that case!
            let (hook_return, parent_ret_deref) = if parent_ret as usize <= 0x100 {
                (false, 0xD3ADB33F as *const usize)
            } else {
                (true, *parent_ret)
            };

            // Save call to global events ringbuffer
            if let Some(events) = &mut EVENTS {
                // Get current globally-unique-event-index
                let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
                if !OVERWRITING.load(Ordering::Relaxed) && cidx >= events.len() - MAX_STACK_HEIGHT {
                    disable();
                    return;
                }

                events[cidx % events.len()] = Event::Entry(Call {
                    time: _rdtsc(),
                    to: child_ret,
                    from: parent_ret_deref,
                    tid,
                });
            }

            // TODO: clean up this hack! we check if we are in mcount, or mcount_entry, mcount_return_tampoline or mcount_return
            if parent_ret_deref >= (mcount as *const usize) && parent_ret_deref <= (rftrace_backend_get_events_index as *const usize) {
                /*unsafe {
                    *(0 as *mut u8) = 0;
                }
                panic!("BLUB!");*/
                //disable();
                // Maybe insert fake end, so uftrace is not confused and crashes because its internal function stack overflows.
                if let Some(events) = &mut EVENTS {
                    let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
                    if !OVERWRITING.load(Ordering::Relaxed) && cidx >= events.len() - MAX_STACK_HEIGHT {
                        disable();
                        return;
                    }

                    events[cidx % events.len()] = Event::Exit(Exit {
                        time: _rdtsc()+20,
                        from: child_ret,
                        tid,
                    });
                }


                return;
            }

            if hook_return {
                let sr = SavedRet {
                    stackloc: parent_ret,
                    retloc: parent_ret_deref,
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
}

#[naked]
pub extern "C" fn mcount_return_trampoline() {
    // does 'nothing', except calling mcount_return. Takes care to not clobber any return registers.
    // based on https://github.com/namhyung/uftrace/blob/master/arch/x86_64/mcount.S

    // System V AMD64 ABI: If the callee wishes to use registers RBX, RBP, and R12–R15, it must restore their original values before returning control to the caller.
    //                     All other registers must be saved by the caller if it wishes to preserve their values.
    // We are in a return trampoline -> we only have to save the registers the return value might be stored in.
    // `call mcount_return` is not allowed to clobber rbx, rbp, ... either, so thats fine.
    // The only issue are interrupts. If we are tracing kernel code, specifically interrupt handlers, we will break stuff since we might change the scratch registers in the middle of a function.
    // To solve this, a compile-time 'interruptsafe' feature is defined, which when set, saves and restores all volatile registers.

    /*
    Stack layout:
        RBP +112
            +104    RETURN-ADDRESS
            +96     r11      |
            +88     r10      |
            +80     r9       |
            +72     r8       |  only when interruptsafe
            +64     rcx      |
            +56     rsi      |
            +48     rdi      |
            +40     xmm1   |
            +32     xmm1   | only when sse2 is available
            +24     xmm0   |
            +16     xmm0   |
            +8      rdx
        RSP +0      rax
    */

    unsafe {
        /* space for locals (saved ret values) (if we dont back up xmm0+1, this is too much, but this won't hurt us) */
        #[cfg(feature = "interruptsafe")]
        llvm_asm!("push %rax"); // fake return value for later
        #[cfg(feature = "interruptsafe")]
        llvm_asm!("pushfq"); // flags for interrupt stuff
        #[cfg(feature = "interruptsafe")]
        llvm_asm!("cli"); // dont do interrupts here!
        #[cfg(feature = "interruptsafe")]
        llvm_asm!("sub $$104, %rsp");
        #[cfg(not(feature = "interruptsafe"))]
        llvm_asm!("sub $$64, %rsp");

        /* always backup return registers */
        llvm_asm!(
            "
            movq   %rdx,   8(%rsp)
            movq   %rax,   0(%rsp)
        "
        );

        /* when we compile against a 'kernel' target we do NOT have sse enabled, otherwise we might. Backup xmm0 and xmm1 */
        /* even if we are in userspace code that could use sse2, we are guaranteed that mcount_return() will not clobber it in this case */
        #[cfg(target_feature = "sse2")]
        llvm_asm!(
            "
            movdqu %xmm0, 16(%rsp)
            movdqu %xmm1, 32(%rsp)
        "
        );

        // If we have to be interrupt safe, also backup non-return scratch registers
        #[cfg(feature = "interruptsafe")]
        llvm_asm!(
            "
            movq   %rdi,   48(%rsp)
            movq   %rsi,   56(%rsp)
            movq   %rcx,   64(%rsp)
            movq   %r8,    72(%rsp)
            movq   %r9,    80(%rsp)
            movq   %r10,   88(%rsp)
            movq   %r11,   96(%rsp)
        "
        );

        llvm_asm!(
            "
            /* set the first argument of mcount_return as pointer to return values */
            movq %rsp, %rdi

            /* call mcount_return, which returns original parent address in rax. */
            call mcount_return
        "
        );

        // Store original parent address at the correct stack location
        #[cfg(not(feature = "interruptsafe"))]
        llvm_asm!(
            "
            movq %rax, 56(%rsp)
        "
        );
        #[cfg(feature = "interruptsafe")]
        llvm_asm!(
            "
            movq %rax, 112(%rsp)
        "
        );

        // restore saved return values
        llvm_asm!(
            "
            movq    0(%rsp), %rax
            movq    8(%rsp), %rdx
        "
        );

        // If we have to be interrupt safe, restore non-return scratch registers
        #[cfg(feature = "interruptsafe")]
        llvm_asm!(
            "
            movq   48(%rsp),   %rdi
            movq   56(%rsp),   %rsi
            movq   64(%rsp),   %rcx
            movq   72(%rsp),   %r8
            movq   80(%rsp),   %r9
            movq   88(%rsp),   %r10
            movq   96(%rsp),   %r11
        "
        );

        /* Restore sse return values, if supported */
        #[cfg(target_feature = "sse2")]
        llvm_asm!(
            "
            movdqu 16(%rsp), %xmm0
            movdqu 32(%rsp), %xmm1
        "
        );

        /* add 8 less back to rsp than we substracted. RET will pop the 'missing' value */
        #[cfg(not(feature = "interruptsafe"))]
        llvm_asm!(
            "
            add $$56, %rsp
        "
        );

        #[cfg(feature = "interruptsafe")]
        llvm_asm!(
            "
            add $$104, %rsp
        "
        ); // here we added same amount back we substracted, since space is in rax push.
        #[cfg(feature = "interruptsafe")]
        llvm_asm!("popfq"); // This should also restore the interrupt flag?
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
    ENABLED.store(false, Ordering::Relaxed);
}

fn enable() {
    ENABLED.store(true, Ordering::Relaxed);
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

// interface, only used by 'parent' rftrace lib this static backend is linked to!

#[no_mangle]
pub extern "C" fn rftrace_backend_get_events_index() -> usize {
    return INDEX.load(Ordering::Relaxed);
}

#[no_mangle]
pub extern "C" fn rftrace_backend_get_events() -> *const Event {
    return unsafe {
        EVENTS
            .take()
            .map(|e| e.as_ptr())
            .unwrap_or(0 as *const Event)
    };
}

#[no_mangle]
pub extern "C" fn rftrace_backend_disable() {
    disable();
}

#[no_mangle]
pub fn rftrace_backend_enable() {
    enable();
}

#[no_mangle]
pub extern "C" fn rftrace_backend_init(bufptr: *mut Event, len: usize, overwriting: bool) {
    let eventbuf = unsafe {
        assert!(!bufptr.is_null());
        slice::from_raw_parts_mut(bufptr, len)
    };

    assert!(
        len > MAX_STACK_HEIGHT,
        "Event buffer has to be larger than maximum stack height!"
    );

    OVERWRITING.store(overwriting, Ordering::Relaxed);

    set_eventbuf(eventbuf);
}
