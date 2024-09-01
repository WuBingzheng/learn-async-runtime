use std::collections::{VecDeque};
use std::pin::Pin;
use std::task::{Context, Waker, Poll, RawWaker, RawWakerVTable};
use std::future::Future;
//use std::sync::{Arc};
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

#[derive(Debug, PartialEq, Eq)]
enum RawTaskState {
    InQueue,
    Running,
    Pending,
    Ready,
    Closed,
    // Cancelled,
}

#[repr(C)]
struct RawTask<T> {
    state: RawTaskState,
    is_join_handle_dropped: bool,
    waker_refs: usize,

    vtable: &'static RawTaskVtable,
    future: Pin<Box<dyn Future<Output = T>>>,
    waker: Option<Waker>,
    output: Option<T>,
}

impl<T> RawTask<T> {
    fn new(future: impl Future<Output = T> + 'static) -> RawTask<T> {
        RawTask {
            state: RawTaskState::InQueue,
            is_join_handle_dropped: false,
            waker_refs: 0,

            vtable: &RawTaskVtable {
                poll_task: Self::poll_task,
            },
            future: Box::pin(future),
            output: None,
            waker: None,
        }
    }

    fn try_drop(ptr: *mut RawTask<T>, from: &'static str) {
        let raw_task = unsafe { &mut *ptr };
        if raw_task.state == RawTaskState::Closed && raw_task.waker_refs == 0 {
            println!("drop task from {}: {:p}", from, ptr);
            unsafe { drop(Box::from_raw(ptr)); }
        }
    }

    fn poll_task(ptr: *mut RawTask<()>) {
        let raw_waker = create_raw_waker(ptr);
        println!(">> newwaker: {:p}: waker_refs++", ptr);
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        let ptr1 = ptr as *mut RawTask<T>;
        let raw_task = unsafe { &mut *ptr1 };

        match raw_task.future.as_mut().poll(&mut cx) {
            Poll::Pending => {
                raw_task.state = RawTaskState::Pending;
                println!("runtime: task {:p} pending", ptr);
            }
            Poll::Ready(output) => {
                println!("runtime: task {:p} done, drop:{},{}", ptr, raw_task.is_join_handle_dropped, raw_task.waker_refs);
                //let task = self.all_tasks.remove(&id).unwrap();

                if raw_task.is_join_handle_dropped {
                    raw_task.state = RawTaskState::Closed;

                    Self::try_drop(ptr1, "poll-ready");
                } else {
                    raw_task.state = RawTaskState::Ready;
                    raw_task.output = Some(output);

                    if let Some(waker) = raw_task.waker.take() {
                        raw_task.state = RawTaskState::Ready;
                        println!("wake JoinHandle");
                        waker.wake();
                    }
                }
            }
        }
    }
}

    const RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        f_clone,
        f_wake,
        f_wake_by_ref,
        f_drop,
    );

    fn create_raw_waker(ptr: *mut RawTask<()>) -> RawWaker {
        let raw_task = unsafe { &mut *ptr };
        raw_task.waker_refs += 1;
        RawWaker::new(ptr as *const (), &RAW_WAKER_VTABLE)
    }
    unsafe fn f_clone(ptr: *const ()) -> RawWaker {
        println!(">> clonewaker {:p}: waker_refs++", ptr);
        create_raw_waker(ptr as *mut RawTask<()>)
    }
    unsafe fn f_wake(ptr: *const ()) {
        f_wake_by_ref(ptr);
        f_drop(ptr); // consume the waker
    }
    unsafe fn f_wake_by_ref(ptr: *const ()) {
        let raw_task = unsafe { &mut *( ptr as *mut RawTask<()>)};
        match &raw_task.state {
            RawTaskState::InQueue => (),
            RawTaskState::Pending | RawTaskState::Running => {
                raw_task.state = RawTaskState::InQueue;
                READY_TASKS.with_borrow_mut(
                    |ready_tasks| ready_tasks.push_back(Task(raw_task)));
            }
            state => {
                todo!("unexpected state: {:?}", state);
            }
        }
    }
    unsafe fn f_drop(ptr: *const ()) {
        let ptr = ptr as *mut RawTask<()>;
        let raw_task = unsafe { &mut *ptr };
        println!(">> {:p}: waker_refs--", ptr);
        raw_task.waker_refs -= 1;
        RawTask::try_drop(ptr, "drop-waker");
    }

// JoinHandle
struct JoinHandle<T>(*mut RawTask<T>);

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let raw_task = unsafe { &mut *self.get_mut().0 };
        match &raw_task.state {
            RawTaskState::Ready => {
                raw_task.state = RawTaskState::Closed;
                Poll::Ready(raw_task.output.take().unwrap())
            }
            RawTaskState::InQueue | RawTaskState::Pending => {
                raw_task.waker = Some(cx.waker().clone());
                Poll::Pending
            }
            _ => {
                todo!();
            }

        }
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        let raw_task = unsafe { &mut *self.0 };
        raw_task.is_join_handle_dropped = true;

            println!("try drop JoinHandle: {:p} {:?} {}", self.0, raw_task.state, raw_task.waker_refs);
        RawTask::try_drop(self.0, "drop-joinhandle");
    }
}

// Task
struct Task(*mut RawTask<()>);


fn spwan<T>(future: impl Future<Output = T> + 'static) -> JoinHandle<T> { // why 'static
    let raw_task = RawTask::new(future);
    let raw_task = Box::leak(Box::new(raw_task));

    // task
    let ptr0 = raw_task as *mut RawTask<T>;
    let ptr2 = ptr0 as *mut RawTask<()>;
    println!("---- new task: {:p}", ptr2);
    let task = Task(ptr2);
    READY_TASKS.with_borrow_mut(
        |ready_tasks| ready_tasks.push_back(task));

    // JoinHandle
    JoinHandle(raw_task)
}

fn run() {
    loop {
        let mut ready_tasks = READY_TASKS.take();
        println!("looooop: ready={}", ready_tasks.len());

        if ready_tasks.is_empty() {
            break;
        }

        while let Some(task) = ready_tasks.pop_front() {
            let raw_task = unsafe { &mut *task.0 };
            raw_task.state = RawTaskState::Running;
            println!("run: {:p} {:p}", raw_task, raw_task.vtable);
            let poll_task = raw_task.vtable.poll_task;
            poll_task(task.0);
        }

    }
    println!("runtime end");
}

fn block_on<T>(future: impl Future<Output = T> + 'static) -> T {
    let join = spwan(future);
    run();

    let raw_task = unsafe { &mut *join.0 };
    raw_task.state = RawTaskState::Closed;
    raw_task.output.take().unwrap()
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
            println!("1111----");
            cx.waker().clone().wake();
            println!("2222----");
            //cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(123)
        }
    }
}

async fn biz_woker() {
    println!("new: r42");
    let _r42 = spwan(r42());

    println!("new: r43");
    spwan(r43());

    println!("new: hello");
    let hello = spwan(hello());

    println!("new: pending-once");
    let r123 = spwan(PendingOnce{get: false});

    println!("new: joinhandles");
    spwan(async { println!("> ret: {}", _r42.await) });
    spwan(async { println!("> ret: {}", hello.await) });
    spwan(async { println!("> ret: {}", r123.await) });
}

fn main()
{
    block_on(biz_woker())
}
