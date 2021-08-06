use std::{collections::{BTreeMap, HashMap}, sync::{Arc, Mutex, mpsc::{Receiver, Sender}}, thread::{self, JoinHandle}, time::Instant};

pub struct Runtime {
    /// Available hreads for the threadpool
    available_threads: Vec<usize>,
    /// Callbacks scheduled to run
    callbacks_to_run: Vec<(usize, Js)>,
    /// All registered callbacks
    callback_queue: HashMap<usize, Box<dyn FnOnce(Js)>>,
    /// Number of pending epoll events
    epoll_pending_events: usize,
    /// Event registrator which registers interest in events with the OS
    epoll_thread: thread::JoinHandle<()>,
    /// None = infinite, Some(n) = timeout in n ms, Some(0) = immediate
    epoll_timeout: Arc<Mutex<Option<i32>>>,
    /// Channel used by both our threadpoll and our epoll thread to send events
    /// to the main loop
    event_reciever: Receiver<PollEvent>,
    /// Creates a unique identity for our callbacks
    identity_token: usize,
    /// Number of events pending. When zero done
    pending_events: usize,
    /// Handles to our threads in the threadpool
    thread_pool: Vec<NodeThread>,
    /// Holds all our timers, and an Id for callback to run once they expire
    timers: BTreeMap<Instant, usize>,
    /// Struct to temporarily hold timers to remove. Let runtime have ownership
    /// so we can reuse the same memory
    timers_to_remove: Vec<Instant>,
}

struct Task {
    task: Box<dyn Fn() -> Js + Send + 'static>,
    callback_id: usize,
    kind: ThreadPoolTaskKind,
}
impl Task {
    /// Cleans up after ourselves and closes down the thread pool
    fn close() -> Self {
        Task {
            task: Box::new(|| Js::Undefined),
            callback_id: 0,
            kind: ThreadPoolTaskKind::Close,
        }
    }
}

struct NodeThread {
    pub(crate) handle: JoinHandle<()>,
    sender: Sender<Task>,
}

pub enum ThreadPoolTaskKind {
    FileRead,
    Encrypt,
    Close,
}

fn main() {
    println!("Hello, world!");
}
