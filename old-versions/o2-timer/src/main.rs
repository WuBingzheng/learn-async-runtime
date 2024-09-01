#![feature(noop_waker)]

use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::pin::Pin;
use std::task::{Context, Waker, Poll, Wake};
use std::future::Future;
use std::sync::{Mutex, Arc};
use std::cmp::Ordering;

use std::time::{SystemTime, Duration};


struct SleepWaker {
    wake_time: SystemTime,
    waker: Waker,
}
impl Ord for SleepWaker {
    fn cmp(&self, other: &Self) -> Ordering {
        self.wake_time.cmp(&other.wake_time).reverse()
    }
}

impl PartialOrd for SleepWaker {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl PartialEq for SleepWaker {
    fn eq(&self, other: &Self) -> bool {
        self.wake_time == other.wake_time
    }
}
impl Eq for SleepWaker {
}


static TIMER: Mutex<BinaryHeap<SleepWaker>> = Mutex::new(BinaryHeap::new());

static READY_TASKS: Mutex<VecDeque<TaskId>> = Mutex::new(VecDeque::new());

// MyWaker
struct MyWaker {
    // _ready_tasks: Arc<VecDeque<TaskId>>, // TODO move here?
    id: TaskId,
}
impl Wake for MyWaker {
    fn wake(self: Arc<Self>) {
        println!("wake: {:?}", self.id);
        let mut ready_tasks = READY_TASKS.lock().unwrap();
        ready_tasks.push_back(self.id);
    }
}


// SleepFuture
struct SleepFuture {
    wake_time: SystemTime,
    poll_cnt: usize,
}

impl Future for SleepFuture {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let sleep_fut = self.get_mut();

        println!("poll sleep: {:?} {:?}", SystemTime::now(), sleep_fut.wake_time);
        if SystemTime::now() >= sleep_fut.wake_time {
            Poll::Ready(sleep_fut.poll_cnt)
        } else {
            sleep_fut.poll_cnt += 1;

            let mut timer = TIMER.lock().unwrap();
            let waker = cx.waker().clone();
            timer.push(SleepWaker{ wake_time:sleep_fut.wake_time, waker});

            Poll::Pending
        }
    }
}

fn sleep(d: Duration) -> impl Future<Output = usize> {
    SleepFuture {
        wake_time: SystemTime::now() + d,
        poll_cnt: 0,
    }
}

// Task
#[derive(Copy, Clone, PartialEq, Eq, Hash, Default, Debug)]
struct TaskId(usize);
struct Task {
    future: Pin<Box<dyn Future<Output = usize>>>,
}

// Runtime
struct Runtime {
    next_id: usize,
    //ready_tasks: VecDeque<TaskId>,
    all_tasks: HashMap<TaskId, Task>,
}

impl Runtime {
    fn new() -> Self {
        Runtime {
            next_id: 0,
            //ready_tasks: VecDeque::new(),
            all_tasks: HashMap::new(),
        }
    }

    fn spwan(&mut self, future: impl Future<Output = usize> + 'static) { // why 'static
        let task = Task { future: Box::pin(future) };

        let id = TaskId(self.next_id);
        self.next_id += 1;

        self.all_tasks.insert(id, task);

        //self.ready_tasks.push_back(id);
        let mut ready_tasks = READY_TASKS.lock().unwrap();
        ready_tasks.push_back(id);
    }

    fn run(&mut self) {
        while !self.all_tasks.is_empty() {
            let mut ready_tasks = READY_TASKS.lock().unwrap();

            println!("looooop: all={} ready={}", self.all_tasks.len(), ready_tasks.len());

            while let Some(id) = ready_tasks.pop_front() {
                let mywaker = MyWaker { id };
                let waker = Arc::new(mywaker).into();
                let mut cx = Context::from_waker(&waker);

                let future = self.all_tasks.get_mut(&id).unwrap().future.as_mut();
                match future.poll(&mut cx) {
                    Poll::Pending => {
                        println!("runtime: task pending");
                        //self.tasks.push_back(task);
                    }
                    Poll::Ready(n) => {
                        self.all_tasks.remove(&id);
                        println!("runtime: task done {n}");
                    }
                }
            }

            drop(ready_tasks);

            let now = SystemTime::now();
            let mut any_ready = false;
            let mut timer = TIMER.lock().unwrap();
            while let Some(sleep_waker) = timer.peek() {
                println!("peek timer: ");
                if sleep_waker.wake_time <= now {
                    any_ready = true;

                    println!("runtime: wake {:?}", sleep_waker.wake_time);

                    let sleep_waker = timer.pop().unwrap();
                    sleep_waker.waker.wake();
                } else {
                    println!("runtime: any_ready: {any_ready}");
                    if !any_ready {
                        println!("runtime: sleep: {:?}", sleep_waker.wake_time.duration_since(now).unwrap());
                        std::thread::sleep(sleep_waker.wake_time.duration_since(now).unwrap());
                    }
                    break;
                }
            }
        }
        println!("runtime end");
    }
}

async fn r42() -> usize {
    42
}

fn main() {
    let mut rt = Runtime::new();
    rt.spwan(r42());
    rt.spwan(sleep(Duration::from_secs(5)));
    rt.spwan(sleep(Duration::from_secs(2)));
    rt.run();
}
