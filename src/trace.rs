use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::Mutex;
use std::cell::RefCell;
use core::arch::x86_64::_rdtsc;
//extern crate byteorder;
use std::fs::File;
use std::io::prelude::*;
use byteorder::{WriteBytesExt, LittleEndian};

static mut ENABLED: bool = false;
static mut INDEX: AtomicUsize = AtomicUsize::new(0);
static mut EVENTS: [Event; MAX_RECORDED_EVENTS] = [Event::Empty; MAX_RECORDED_EVENTS];

const MAX_STACK_HEIGHT: usize = 1000;
const MAX_RECORDED_EVENTS: usize = 1000000;

/// Mutex, which we use to determine if ANY thread is currently initializing. TODO: thread-local this?
/// Needed, since we have to disable() to avoid infinite recursion on alloc of new-thread's retstack vec
static mut CURRENTLY_INIT: AtomicBool = AtomicBool::new(false);

// Issue: thread_local allocs on first access. I have no way of detecting first use per thread.
// When it allocs it goes into infinite recursion, since kernel is used for alloc, which is hooked with mcount().
// mcount then inits it again.
// Solution: use THREAD_INIT to determine if inited, use a global disable-lock CURRENTLY_INIT
thread_local! {
    static RETSTACK: Box<RetStack> = Box::new(RetStack::new(MAX_STACK_HEIGHT));
}

/// Determines whether a thread is already initialited, ie it is safe to access RETSTACK, no recursion!
#[thread_local]
static mut THREAD_INIT: bool = false;


#[derive(Debug, Clone, Copy)]
enum Event {
    Empty,
    Entry(Call),
    Exit(Exit)
}

#[derive(Debug, Clone, Copy)]
struct Call {
    time: u64,
    from: *const usize,
    to: *const usize,
}

#[derive(Debug, Clone, Copy)]
struct Exit {
    time: u64,
    from: *const usize,
}
struct RetStack {
    vec: RefCell<Vec<SavedRet>>,
    capacity: usize,
}

impl RetStack {
    pub fn new(capacity: usize) -> RetStack {
        println!("Creating retstack...!");
        RetStack{vec: RefCell::new(Vec::with_capacity(capacity)), capacity}
    }

    pub fn push(&self, item: SavedRet) {
        let mut vec = self.vec.borrow_mut();
        if (vec.len() >= self.capacity) {
            panic!("RetStack full!");
        }
        vec.push(item);
    }

    pub fn pop(&self) -> Option<SavedRet> {
        let mut vec = self.vec.borrow_mut();
        vec.pop()
    }
}

#[derive(Debug, Clone, Copy)]
struct SavedRet {
    stackloc: *mut *const usize,
    retloc: *const usize,
    childip: *const usize,
}


// magic from https://stackoverflow.com/questions/54999851/how-do-i-get-the-return-address-of-a-function/56308426#56308426
// We use llvm compiler intrinsic to get return address without having to know target / write asm
// https://llvm.org/docs/LangRef.html
extern {
    // declare i8* @llvm.returnaddress(i32 <level>)
    #[link_name = "llvm.returnaddress"]
    fn return_address(_:i32) -> *const i8;
    
    // declare i8* @llvm.addressofreturnaddress()
    #[link_name = "llvm.addressofreturnaddress"]
    fn address_of_return_address() -> *const i8;
}


#[naked]
#[no_mangle]
pub extern "C" fn mcount() {
    // We need to be careful with hooked naked functions!
    // Normally, llvm ensures that all needed functions parameters are saved before the embedded mcount() is called, and restored afterwards.
    // This does NOT happen with naked funktions like `hermit::arch::x86_64::kernel::switch::switch:`
    // There, the args (like old and new_stack) are clobbered. This is because they are not used, only implicitly in the asm! code, so llvm does not know they are used!
    // To be sure the instrumentation never breaks anything, we backup and restore any possible argument registers
    // TODO: Implement feature to skip this, which can be enabled if we are sure this can't happen with the code we are instrumenting?

    // we need custom assembly that "knows" that mcount is ALWAYS called at the start of each function! no llvm magic can help here.
    // parents-return-addr is always stored at rbp+8
    // mcounts ret addr is directly at rsp

    // based on https://github.com/namhyung/uftrace/blob/master/arch/x86_64/mcount.S
    unsafe{
        if !ENABLED {
            return;
        } 
        asm!("
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
    // cannot use anything that calls a function in the kernel here!
    // Else we will have an infinite recursion!
    // OR: we temp disable. Multithread disabling guarded by CURRENTLY_INIT.
    // NO PRINTING IN MCOUNT! Even if disabling before! (might be holding relevant lock or ref, so print() cannot succeed!)

    unsafe {
        if ENABLED {
            let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
            EVENTS[cidx % MAX_RECORDED_EVENTS] = Event::Entry(Call{time: _rdtsc(), to: child_ret, from: *parent_ret});
            /*if cidx > 90000 {
                disable();
            }*/

            // Avoid recursion on instanciating the lazy thread-local RETSTACK
            if !THREAD_INIT {
                // cannot define CURRENTLY_INIT as mutex and use something like
                //let lock = CURRENTLY_INIT.try_lock(); 
                // since it uses kernel -> might recurse! --> use atomics instead.
                //
                // set currently init to true. If it was true, do nothing. Else init ourselves.
                if !CURRENTLY_INIT.swap(true, Ordering::Relaxed) {
                    disable();
                    // access retstack once, so it gets lazy-initialized and allocated!
                    RETSTACK.with(|s| return);
                    THREAD_INIT = true;

                    CURRENTLY_INIT.store(false, Ordering::Relaxed);
                    // TODO/FIXME: we might be interrupted exactly here.
                    // another thread goes disable(), we go enable while it is initializing.
                    enable();
                }
            } else {
                // We are enabled and initialized, redirect return pointer to mcount_return_trampoline!
                RETSTACK.with(|stack| {
                    let sr = SavedRet{stackloc: parent_ret, retloc: *parent_ret, childip: child_ret};
                    stack.push(sr);
                    *parent_ret = mcount_return_trampoline as *const usize;
                });
            }
        }
    }
}


#[naked]
pub extern "C" fn mcount_return_trampoline() {
    // does 'nothing', except calling mcount_return. Takes care to not clobber any return registers.
    // based on https://github.com/namhyung/uftrace/blob/master/arch/x86_64/mcount.S

    unsafe{
        asm!("
            /* space for locals (saved ret values) */
            sub $$48, %rsp

            /* save registers which could contain return values (missing xmm1 for full wikipedia/systemv compliance?) */
            movdqu %xmm0, 16(%rsp)
            movq   %rdx,   8(%rsp)
            movq   %rax,   0(%rsp)

            /* set the first argument of mcount_return as pointer to return values */
            movq %rsp, %rdi

            /* call mcount_return, which returns original parent address. Store it at the correct stack location */
            call mcount_return
            movq %rax, 40(%rsp)

            /* restore saved return values */
            movq    0(%rsp), %rax
            movq    8(%rsp), %rdx
            movdqu 16(%rsp), %xmm0

            /* add only 40 to rsp, so the missing 8 become the new return pointer */
            add $$40, %rsp
            retq
        ");
    }
}


#[no_mangle]
pub extern "C" fn mcount_return() -> *const usize {
    unsafe {
        if !THREAD_INIT {
            disable();
            println!("Returned without initializing thread!");
            panic!("Returned without initializing thread!");
        }

        let ret = address_of_return_address() as *mut *const usize;
        let (original_ret, childip) = RETSTACK.with(|stack| {
            let sr = stack.pop().expect("retstack empty?");

            // Sanity check return location. trampoline has 48 byte stack offset.
            #[cfg(debug_assertions)]
            {
                if sr.stackloc as usize != ret as usize +48 {
                    disable();
                    println!("Stack frame misalignment: {:?} {:?}", sr, ret);
                    //println!("Missing x stack frames: {}", stack.len());
                    while let Some(s) = stack.pop() {
                        println!("{:?}", s);
                    }
                    panic!("Stack frame misalignment!");
                }
                if sr.retloc == mcount_return_trampoline as *const usize {
                    // This happens when an interrupt interrupts while we are in mcount_return_trampoline?
                    // But why is this very exact? hmm
                    //disable();
                    /*println!("Returning to mcount_return_trampoline. How does that happend?!");
                    while let Some(s) = stack.pop() {
                        println!("{:?}", s);
                    }
                    panic!("Return to mcount_return_trampoline!");*/

                }

            }
            (sr.retloc, sr.childip)
        });

        let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
        EVENTS[cidx % MAX_RECORDED_EVENTS] = Event::Exit(Exit{time: _rdtsc(), from: childip});

        original_ret
    }
}


pub fn print() {
    disable();
    for c in unsafe{&EVENTS[0..50]} {
        println!("{:?}", c);
    }
    // this is only current task, for debug!
    RETSTACK.with(|stack| {
        while let Some(s) = stack.pop() {
            println!("{:?}", s);
        }
    });
}

pub fn disable() {
    unsafe{ENABLED = false;}
}

pub fn enable() {
    println!("enabling mcount hooks..");
    unsafe{ENABLED = true;}
}

pub fn dump_file_uftrace() {
    // Uftraces trace format: a bunch of 64-bit fields.
    // two 64bit for one event. See https://github.com/namhyung/uftrace/wiki/Data-Format
    // 
    /* struct uftrace_record {
        uint64_t time;
        uint64_t type:   2;
        uint64_t more:   1;
        uint64_t magic:  3;
        uint64_t depth:  10;
        uint64_t addr:   48; /* child ip or uftrace_event_id */
    }; */
    // TODO: create enable lock?
    disable();
    println!("Saving trace to disk...!");
    let mut out = Vec::<u8>::new();

    let cidx = unsafe{INDEX.load(Ordering::Relaxed)} % MAX_RECORDED_EVENTS;
    for e in unsafe{EVENTS[cidx..].iter().chain(EVENTS[..cidx].iter())} {
        match e {
            Event::Exit(e) => {
                out.write_u64::<LittleEndian>(e.time);
        
                let mut addr = 0;
                addr |= 1 << 0; // type = UFTRACE_EXIT
                addr |= 0 << 2; // more, always 0
                addr |= 0b101 << 3; // magic, always 0b101
                addr |= (0 & ((1<<10) - 1)) << 6; // depth
                addr |= (e.from as u64 & ((1<<48)-1)) << 16; // actual address, limited to 48 bit.
                out.write_u64::<LittleEndian>(addr);
            },
            Event::Entry(e) => {
                out.write_u64::<LittleEndian>(e.time);
        
                let mut addr = 0;
                addr |= 0 << 0; // type = UFTRACE_ENTRY
                addr |= 0 << 2; // more, always 0
                addr |= 0b101 << 3; // magic, always 0b101
                addr |= (0 & ((1<<10) - 1)) << 6; // depth
                addr |= (e.to as u64 & ((1<<48)-1)) << 16; // actual address, limited to 48 bit.
                out.write_u64::<LittleEndian>(addr);
            }
            Event::Empty => {
                // Reached end of populated entries.
                //println!("No further events found!");
                continue;
            }
        }
    }
    println!("Writing to disk: {} events", out.len());
    // TODO: error handling
    let mut file = File::create("/myfs/trace.dat").expect("Could not create trace file!");
    file.write_all(&out[..]).expect("Could not write trace file");
}