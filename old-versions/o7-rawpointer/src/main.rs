use std::collections::{VecDeque};
use std::pin::Pin;
use std::task::{Context, Waker, Poll, Wake};
use std::future::Future;
use std::sync::{Arc};
//use std::rc::Rc;
//use std::time::Duration;
use std::cell::RefCell;


thread_local! {
    static READY_TASKS: RefCell<VecDeque<Task>> = RefCell::new(VecDeque::new());
}


// RawTask
struct RawTaskVtable {
    poll_task: fn(*mut RawTask<()>),
}

#[repr(C)]
struct RawTask<T> {
    vtable: &'static RawTaskVtable,
    future: Pin<Box<dyn Future<Output = T>>>,
    waker: Option<Waker>,
    output: Option<T>,
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

    fn poll_task(ptr: *mut RawTask<()>) {
        let mywaker = MyWaker(ptr);
        let waker = Arc::new(mywaker).into();
        let mut cx = Context::from_waker(&waker);

        let ptr = ptr as *mut RawTask<T>;
        let raw_task = unsafe { &mut *ptr };

        match raw_task.future.as_mut().poll(&mut cx) {
            Poll::Pending => {
                println!("runtime: task {:p} pending", ptr);
            }
            Poll::Ready(output) => {
                println!("runtime: task {:p} done", ptr);
                //let task = self.all_tasks.remove(&id).unwrap();

                raw_task.output = Some(output);

                if let Some(waker) = raw_task.waker.take() {
                    println!("wake JoinHandle");
                    waker.wake();
                }
            }
        }
    }
}


// MyWaker
struct MyWaker(*mut RawTask<()>);

unsafe impl Send for MyWaker {}
unsafe impl Sync for MyWaker {}

impl Wake for MyWaker {
    fn wake(self: Arc<Self>) {
        READY_TASKS.with_borrow_mut(
            |ready_tasks| ready_tasks.push_back(Task(self.0)));
    }
}

// JoinHandle
struct JoinHandle<T>(*mut RawTask<T>);

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let raw_task = unsafe { &mut *self.get_mut().0 };
        match raw_task.output.take() {
            Some(output) => {
                println!("JoinHandle ready");
                Poll::Ready(output)
            }
            None => {
                println!("JoinHandle pending !!!!!!");
                raw_task.waker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

// Task
struct Task(*mut RawTask<()>);

// Runtime
struct Runtime {
}

impl Runtime {
    fn new() -> Self {
        Runtime {
        }
    }

    fn spwan<T>(&mut self, future: impl Future<Output = T> + 'static) -> JoinHandle<T> { // why 'static
        let raw_task = RawTask::new(future);
        let raw_task = Box::leak(Box::new(raw_task));

        // task
        let ptr0 = raw_task as *mut RawTask<T>;
        let ptr2 = ptr0 as *mut RawTask<()>;
        let task = Task(ptr2);
        READY_TASKS.with_borrow_mut(
            |ready_tasks| ready_tasks.push_back(task));

        // JoinHandle
        JoinHandle(raw_task)
    }

    fn run(&mut self) {
        loop {
            let mut ready_tasks = READY_TASKS.take();
            println!("looooop: ready={}", ready_tasks.len());

            if ready_tasks.is_empty() {
                break;
            }

            while let Some(task) = ready_tasks.pop_front() {
                let raw_task = unsafe { &mut *task.0 };
                println!("run: {:p} {:p}", raw_task, raw_task.vtable);
                let poll_task = raw_task.vtable.poll_task;
                poll_task(task.0);
            }

        }
        println!("runtime end");
    }
}

async fn r42() -> usize {
    42
}
async fn r43() -> usize {
    43
}
async fn hello() -> String {
    String::from("hello, world")
}

struct PendingOnce {
    get: bool,
}

impl Future for PendingOnce {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        if !fut.get {
            fut.get = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(123)
        }
    }
}

fn main() {
    let mut rt = Runtime::new();
    let _r42 = rt.spwan(r42());
    rt.spwan(r43());
    let hello = rt.spwan(hello());

    let r123 = rt.spwan(PendingOnce{get: false});

    rt.spwan(async { println!("============>>>> ret: {}", _r42.await) });
    rt.spwan(async { println!("============>>>> ret: {}", hello.await) });
    rt.spwan(async { println!("============>>>> ret: {}", r123.await) });

    rt.run();
}
