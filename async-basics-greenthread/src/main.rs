#![feature(llvm_asm)]
#![feature(naked_functions)]
#![feature(asm)]

use std::io::Write;
use std::ptr;

const DEFAULT_STACK_SIZE: usize = 1024 * 1024 * 2;
const MAX_THREADS: usize = 4;
static mut RUNTIME: usize = 0;

pub struct Runtime {
    threads: Vec<Thread>,
    current: usize,
}
impl Runtime {
    pub fn new() -> Self {
        // Create a new base running thread with id of 0
        // And the thread is Running
        let base_thread = Thread {
            id: 0,
            stack: vec![0u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Running,
        };

        // Create a vector with the base thread
        let mut threads = vec![base_thread];
        
        // Create the spawned threads with id from 1 to MAX_THREADS
        let mut available_threads: Vec<Thread> = (1..MAX_THREADS).map(|i| Thread::new(i)).collect();
        
        // Append to vector of threads
        threads.append(&mut available_threads);

        // Return the new runtime with the current thread as 0 (base thread)
        Runtime {
            threads,
            current: 0,
        }
    }

    pub fn init(&self) {
        // Cheating, but need a pointer to our Runtime
        // stored so we can call yield on it even if we don't have
        // a reference to it
        unsafe {
            let r_ptr: *const Runtime = self;
            RUNTIME = r_ptr as usize;
        }
    }

    pub fn run(&mut self) -> ! {
        // Continuosuly call t_yield() until returns false
        while self.t_yield() {}

        // Exit when t_yield() returns false
        std::process::exit(0);
    }

    /// Call when thread is finished
    /// User does not call this, stack calls it when thread is done
    /// If calling thread is base_thread don't do anything. Runtime
    /// calls yield for us on the base thread.
    /// If from spawned thread, we know it's finished (from guard). So
    /// set its state to Available.
    /// Then call t_yield which schedules a new thried to be run
    fn t_return(&mut self) {
        if self.current != 0 {
            self.threads[self.current].state = State::Available;
            self.t_yield();
        }
    }

    fn t_yield(&mut self) -> bool {
        let mut pos = self.current;

        // Check all threads until find one that Ready
        // If none are Ready then return false. (we are done, returning false
        // breaks the run loop and exits).
        while self.threads[pos].state != State::Ready {
            pos += 1;
            if pos == self.threads.len() {
                pos = 0;
            }
            if pos == self.current {
                return false;
            }
        }

        // Change state of current thread from Running to Ready
        if self.threads[self.current].state != State::Available {
            self.threads[self.current].state = State::Ready;
        }

        // The new thread state changes from Ready to Running
        self.threads[pos].state = State::Running;

        // Store the old/current thread index
        let old_pos = self.current;
        // Set the current thread index to the new thread
        self.current = pos;

        unsafe {
            // Get pointers to the old and new ThreadContext
            let old: *mut ThreadContext = &mut self.threads[old_pos].ctx;
            let new: *const ThreadContext = &self.threads[pos].ctx;

            // Save the current context (aka old ThreadContext)
            // Load the new context (aka current ThreadContext) into the CPU
            // %rdi is the 1st argument to functions (Linux and MacOS) and is the old context
            // $rsi is the 2nd argument to functions (Linux and MacOS) and is the new context
            // rdi and rsi are used to pass as arguments into switch as it is a naked function
            asm!(
                "mov rdi, {0}",
                "mov rsi, {1}",
                in(reg) old,
                in(reg) new,
            );

            // Naked function that switches the contexts
            switch();
        }
        
        // Prevents compiler from optimizing code away
        self.threads.len() > 0
    }

    pub fn spawn(&mut self, f: fn()) {
        // Find the first available thread. Panic if no threads are available
        let available = self
            .threads
            .iter_mut()
            .find(|t| t.state == State::Available)
            .expect("no available thread.");
        
        // Get the size of the available thread's stack
        let size = available.stack.len();

        // Initialize a stack according to the psABI stack layout
        unsafe {
            // Pointer to u8 byte array (the stack).
            // As the stack grows downward we start from the highest address point
            // AKA the top of the stack. Thus offset it by its size
            let s_ptr = available.stack.as_mut_ptr().offset(size as isize);
            // Ensure it is 16-byte aligned
            let s_ptr = (s_ptr as usize & !15) as *mut u8;
            // Subtract 16 bytes to write the pointer to our guard function
            // In order to satisfy the 16 byte boundary
            std::ptr::write(s_ptr.offset(-16) as *mut u64, guard as u64);
            // Subtract 8 bytes to write the pointer to our skip function
            // To handle the gap when we return from f so that guard gets called on 16 byte boundary
            std::ptr::write(s_ptr.offset(-24) as *mut u64, skip as u64);
            // Write address to our function at the next 16 byte boundary
            std::ptr::write(s_ptr.offset(-32) as *mut u64, f as u64);
            // Set the spawned thread's context rsp (stack pointer) to function so runs function, then skip, then guard
            available.ctx.rsp = s_ptr.offset(-32) as u64;
        }
        // Now available thread has work to do.
        // Set the current state to ready to work.
        // So Scheduler actually starts up this thread
        available.state = State::Ready;
    }
}

/// Means function we passed in has returned and our thread is finished running its task
/// So de-reference runtime and call t_return()
fn guard() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_return();
    }
}

// Use #[naked] so essentially compiles to just the ret instruction
// ret pops off the next value from the stack and jumps to whatever instructions that address
// points to (our guard function)
#[naked]
unsafe extern "C" fn skip() {
    asm!("ret", options(noreturn));
}

/// Lets us call yield from an arbitrary place in our code. Unsafe though
/// If we call this as Runtime is not initialized yet or runtime is dropped results
/// in undefined behavior
pub fn yield_thread() {
    unsafe {
        let rt_ptr = RUNTIME as *mut Runtime;
        (*rt_ptr).t_yield();
    }
}

// Pass address to our old an new threadcontext using assembly
// %rdi is old ThreadContext, %rsi is new ThreadContext
// Move existing values into our old thread context (save old context)
// Move new values into registers (load new context)

#[naked]
#[inline(never)]
unsafe extern "C" fn switch() {
    asm!(
        "mov [rdi + 0x00], rsp",
        "mov [rdi + 0x08], r15",
        "mov [rdi + 0x10], r14",
        "mov [rdi + 0x18], r13",
        "mov [rdi + 0x20], r12",
        "mov [rdi + 0x28], rbx",
        "mov [rdi + 0x30], rbp",
    
        "mov rsp, [rsi + 0x00]",
        "mov r15, [rsi + 0x08]",
        "mov r14, [rsi + 0x10]",
        "mov r13, [rsi + 0x18]",
        "mov r12, [rsi + 0x20]",
        "mov rbx, [rsi + 0x28]",
        "mov rbp, [rsi + 0x30]",
        "ret",
        options(noreturn)
    )
}


#[derive(PartialEq, Eq, Debug)]
enum State {
    Available,
    Running,
    Ready,
}

struct Thread {
    id: usize,
    stack: Vec<u8>,
    ctx: ThreadContext,
    state: State,
}
impl Thread {
    fn new(id: usize) -> Self {
        Thread {
            id,
            stack: vec![0u8; DEFAULT_STACK_SIZE],
            ctx: ThreadContext::default(),
            state: State::Available,
        }
    }
}

#[derive(Debug, Default)]
#[repr(C)]
struct ThreadContext {
    rsp: u64,
    r15: u64,
    r14: u64,
    r13: u64,
    r12: u64,
    rbx: u64,
    rpb: u64,
}

fn main() {
    // Create a new runtime
    let mut runtime = Runtime::new();

    // Initialize the pointer to the runtime
    runtime.init();

    // Spawn a new thread
    runtime.spawn(|| {
        println!("THREAD 1 STARTING");
        let id = 1;
        for i in 0..10 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
        println!("THREAD 1 FINISHED");
    });
    runtime.spawn(|| {
        println!("THREAD 2 STARTING");
        let id = 2;
        for i in 0..15 {
            println!("thread: {} counter: {}", id, i);
            yield_thread();
        }
        println!("THREAD 2 FINISHED");
    });
    runtime.run();
}
