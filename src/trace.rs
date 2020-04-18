use std::sync::atomic::{AtomicUsize, AtomicBool, Ordering};
use std::sync::Mutex;
//use crossbeam::queue::ArrayQueue;
use std::cell::RefCell;

#[derive(Debug, Clone, Copy)]
enum Calltype {
    Entry,
    Exit
}

#[derive(Debug, Clone, Copy)]
struct Call {
    ctype: Calltype,
    time: u64,
    child: usize,
    parent: usize,
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

static mut ENABLED: bool = false;
static mut INDEX: AtomicUsize = AtomicUsize::new(0);
static mut CALLS: [Call; 10000] = [Call{ctype: Calltype::Entry, time:0, child: 0, parent: 0}; 10000];

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
        return vec.pop() //.expect("RetStack empty!");
    }
}

#[derive(Debug, Clone, Copy)]
struct SavedRet {
    stackloc: *mut *const usize,
    retloc: usize,
}

/// Mutex, which we use to determine if ANY thread is currently initializing. TODO: thread-local this?
/// Needed, since we have to disable() to avoid infinite recursion on alloc of new-thread's retstack vec
static mut CURRENTLY_INIT: AtomicBool = AtomicBool::new(false);

// Issue: thread_local allocs on first access. I have no way of detecting first use per thread.
// When it allocs it goes into infinite recursion, since kernel is used for alloc, which is hooked with mcount().
// mcount then inits it again.
// Solution: use THREAD_INIT to determine if inited, use a global disable-lock CURRENTLY_INIT
thread_local! {
    static RETSTACK: RetStack = RetStack::new(1000);
}

/// Determines whether a thread is already initialited, ie it is safe to access RETSTACK, no recursion!
#[thread_local]
static mut THREAD_INIT: bool = false;


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

    // from https://github.com/namhyung/uftrace/blob/master/arch/x86_64/mcount.S
    unsafe{
        if !ENABLED {
            return;
        } 
        asm!("
        sub $$48, %rsp

        /* save register arguments in mcount_args */
        movq %rdi, 40(%rsp)
        movq %rsi, 32(%rsp)
        movq %rdx, 24(%rsp)
        movq %rcx, 16(%rsp)
        movq %r8,   8(%rsp)
        movq %r9,   0(%rsp)

        /* child addr */
        movq 48(%rsp), %rsi

        /* parent location */
        lea 8(%rbp), %rdi

        /* mcount_args */
        movq %rsp, %rdx

        /* align stack pointer to 16-byte */
        andq $$0xfffffffffffffff0, %rsp
        push %rdx

        /* save rax (implicit argument for variadic functions) */
        push %rax

        call mcount_entry

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

        add $$48, %rsp
        retq
        "); 
    }
}


#[no_mangle]
pub extern "C" fn mcount_entry(parent_ret: *mut *const usize, child_ret: *mut *const usize) {
    // cannot use anything that calls a function in the kernel here!
    // Else we will have an infinite recursion!
    // OR: we temp disable. Multithread disabling guarded by CURRENTLY_INIT.

    unsafe {
        if ENABLED {
            let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
            CALLS[cidx % 10000] = Call {ctype: Calltype::Entry, time: 0, child: (child_ret as usize), parent: (parent_ret as usize)};

            // Avoid recursion on instanciating the lazy thread-local RETSTACK
            // does NOT help, since initial access to INITIALIZED already borks it.
            if !THREAD_INIT {
                // cannot define CURRENTLY_INIT as mutex and use something like
                //let lock = CURRENTLY_INIT.try_lock(); 
                // since it uses kernel -> might recurse!
                
                // set currently init to true. If it was true, do nothing. Else init ourselves.
                if !CURRENTLY_INIT.swap(true, Ordering::Relaxed) {
                    disable();
                    // access retstack once, so it gets lazy-initialized and allocated!
                    RETSTACK.with(|s| return);
                    THREAD_INIT = true;

                    CURRENTLY_INIT.store(false, Ordering::Relaxed);
                    // FIXME: we might be interrupted exactly here.
                    // another thread goes disable(), we go enable while it is initializing.
                    enable();
                }
            } else {
                // We are enabled and initialized, redirect return pointer to mcount_return_trampoline!
                RETSTACK.with(|stack| {
                    let sr = SavedRet{stackloc: parent_ret, retloc: *parent_ret as usize};
                    stack.push(sr);
                    *parent_ret = mcount_return_trampoline as *const usize;
                });
            }
        }
    }
}


#[naked]
pub extern "C" fn mcount_return_trampoline() {
    // does nothing, except calling mcount_return. So we can easily modify its return pointer to the original functions one.

    //unsafe{asm!("call mcount_return;")};
    //or:
    mcount_return();
    panic!(); // should never return! needed to avoid optimization that call is converted to a jmp. We need the push from the call!
}


#[no_mangle]
pub extern "C" fn mcount_return() {
    unsafe {

        let cidx = INDEX.fetch_add(1, Ordering::Relaxed);
        CALLS[cidx % 10000] = Call {ctype: Calltype::Exit, time: 0, child: 0, parent: 0};
        
        let ret = address_of_return_address() as *mut *const usize;
        *ret = RETSTACK.with(|stack| {
            let sr = stack.pop().expect("retstack empty?");

            // Sanity check return location.
            if sr.stackloc != ret {
                disable();
                println!("Stack frame misalignment: {:?} {:?} {:?}", sr, ret, *ret);
                //println!("Missing x stack frames: {}", stack.len());
                while let Some(s) = stack.pop() {
                    println!("{:?}", s);
                }
                panic!("Stack frame misalignment!");
            }
            sr.retloc as *const usize
        });
    }
}


pub fn print() {
    disable();
    for c in unsafe{&CALLS[0..50]} {
        println!("Call: {:?}", c);
    }
}

pub fn disable() {
    unsafe{ENABLED = false;}
    RETSTACK.with(|s| return);
}

pub fn enable() {
    println!("enabling mcount hooks..");
    RETSTACK.with(|s| return);
    unsafe{ENABLED = true;}
}