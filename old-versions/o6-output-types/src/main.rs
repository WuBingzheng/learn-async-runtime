use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::task::{Context, Waker, Poll, Wake};
use std::future::Future;
use std::sync::{Arc};
use std::rc::Rc;
//use std::time::Duration;
use std::cell::RefCell;
use std::sync::atomic::{self, AtomicBool};

mod channel;
mod sleep;

struct TaskId(usize);

thread_local! {
    static READY_TASKS: RefCell<VecDeque<usize>> = RefCell::new(VecDeque::new());
    static ALL_TASKS: RefCell<HashMap<usize, Task>> = RefCell::new(HashMap::new());
}

static NEW_READY: AtomicBool = AtomicBool::new(false);

// MyWaker
struct MyWaker(TaskId);

impl Wake for MyWaker {
    fn wake(self: Arc<Self>) {
        READY_TASKS.with_borrow_mut(
            |ready_tasks| ready_tasks.push_back(self.0.0));

        NEW_READY.store(true, atomic::Ordering::Relaxed);
    }
}

// JoinHandle
#[derive(Clone)]
struct JoinHandle<T>(Rc<RefCell<RawTask<T>>>);

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        match self.0.borrow_mut().output.take() {
            Some(output) => Poll::Ready(output),
            None => {
                let mut raw_task = self.0.borrow_mut();
                raw_task.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

// RawTask
struct RawTaskVtable {
    poll_task: fn(Task, usize),
}

#[repr(C)]
struct RawTask<T> {
    vtable: &'static RawTaskVtable,
    future: Pin<Box<dyn Future<Output = T>>>,
    output: Option<T>,
    waker: Option<Waker>,
}

impl<T> RawTask<T> {
    fn new(future: impl Future<Output = T> + 'static) -> RawTask<T> {
        RawTask {
            vtable: &RawTaskVtable {
                poll_task: Self::poll_task,
            },
            future: Box::pin(future),
            output: None,
            waker: None,
        }
    }

    fn poll_task(task: Task, id: usize) {

        let mywaker = MyWaker ( TaskId(id) );
        let waker = Arc::new(mywaker).into();
        let mut cx = Context::from_waker(&waker);

        let raw_task = Rc::into_raw(task.raw_task);
        let raw_task = unsafe { Rc::from_raw(raw_task as *mut RefCell<RawTask<T>>) };

        let mut raw_task = raw_task.borrow_mut();
        let future = raw_task.future.as_mut();
        match future.poll(&mut cx) {
            Poll::Pending => {
                println!("runtime: task {:?} pending", 0);
            }
            Poll::Ready(output) => {
                println!("runtime: task {:?} done", 0);
                //let task = self.all_tasks.remove(&id).unwrap();

                raw_task.output = Some(output);

                if let Some(waker) = raw_task.waker.take() {
                    waker.wake();
                }
            }
        }
    }
}


// Task
struct Task {
    raw_task: Rc<RefCell<RawTask<()>>>,
}

// Runtime
struct Runtime {
    next_id: usize,
}

impl Runtime {
    fn new() -> Self {
        Runtime {
            next_id: 0,
        }
    }

    fn spwan<T>(&mut self, future: impl Future<Output = T> + 'static) -> JoinHandle<T> { // why 'static
        let id = self.next_id;
        self.next_id += 1;

        let raw_task = Rc::new(RefCell::new(RawTask::new(future)));

        // task
        let notype_task = Rc::into_raw(raw_task.clone());
        let notype_task = unsafe { Rc::from_raw(notype_task as *mut RefCell<RawTask<()>>) };

        let task = Task { raw_task: notype_task };
        ALL_TASKS.with_borrow_mut(
            |all_tasks| all_tasks.insert(id, task));

        READY_TASKS.with_borrow_mut(
            |ready_tasks| ready_tasks.push_back(id));

        // JoinHandle
        JoinHandle ( raw_task )
    }

    fn run(&mut self) {
        ALL_TASKS.with_borrow_mut(
            |all_tasks| 

        while !all_tasks.is_empty() {
            NEW_READY.store(false, atomic::Ordering::Relaxed);

            let mut ready_tasks = READY_TASKS.take();
            println!("looooop: all={} ready={}", all_tasks.len(), ready_tasks.len());

            while let Some(id) = ready_tasks.pop_front() {

                let task = all_tasks.remove(&id).unwrap();
                let poll_task = task.raw_task.borrow().vtable.poll_task;
                poll_task(task, id);

                /*
                let mywaker = MyWaker { id };
                let waker = Arc::new(mywaker).into();
                let mut cx = Context::from_waker(&waker);

                let future = self.all_tasks.get_mut(&id).unwrap().raw_task.future.as_mut();
                match future.poll(&mut cx) {
                    Poll::Pending => {
                        println!("runtime: task {:?} pending", id);
                        //self.tasks.push_back(task);
                    }
                    Poll::Ready(()) => {
                        println!("runtime: task {:?} done", id);
                        let task = self.all_tasks.remove(&id).unwrap();

                        let mut join_inner = task.join_handle.0.borrow_mut();
                        join_inner.finished = true;

                        if let Some(waker) = join_inner.waker.take() {
                            waker.wake();
                        }
                    }
                }
                */
            }

            if let Some(next_time) = sleep::timer_poll() {
                if !NEW_READY.load(atomic::Ordering::Relaxed) {
                    println!("runtime: sleep: {:?}", next_time);
                    std::thread::sleep(next_time);
                }
            }
        });
        println!("runtime end");
    }
}

async fn r42() -> usize {
    42
}
async fn hello() -> String {
    String::from("hello, world")
}

fn main() {
    let mut rt = Runtime::new();
    let _r42 = rt.spwan(r42());
    let hello = rt.spwan(hello());

    rt.spwan(async { println!("============>>>> ret: {}", _r42.await); 0 });
    rt.spwan(async { println!("============>>>> ret: {}", hello.await); 0 });

    rt.run();
}
