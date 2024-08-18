use std::cell::RefCell;
use std::collections::VecDeque;
use std::future::Future;
use std::mem::ManuallyDrop;
use std::ptr::drop_in_place;
use std::pin::Pin;
use std::task::{Context, Waker, Poll, RawWaker, RawWakerVTable};

use crate::event;

// RawTask
struct RawTaskVtable {
    poll_task: fn(*mut Header),
}

//
// The state machine:
//
//   --0--> Active --1--> Running --4--+--5--> Ready --6--> Closed
//              ^            |         |                      ^
//               \           2         |----------7----------/
//                \          |
//                 \         V
//                  --3-- Pending
//
// 0. start as Active, waiting for be polled
// 1. poll
// 2. poll() returns Poll::Pending
// 3. be waked up by wakers
// 4. poll() returns Poll::Ready(T)
// 5. there is JoinHandle, save the output
// 6. JoinHandle cosumes the output
// 7. there is no JoinHandle
//
// There is no Panic or Cancelled.
//
#[derive(Debug, PartialEq, Eq)]
enum TaskState {
    Active,
    Running,
    Pending,
    Ready,
    Closed,
}

/// Header part.
/// Independent from the types of Future and Output, so can be used
/// to define the `ACTIVE_TASKS` queue.
struct Header {
    state: TaskState,
    detached: bool,
    waker_refs: u32,

    vtable: &'static RawTaskVtable,
    awaker: Option<Waker>,
}

/// Body part.
/// The Future (before ready) or the Output (after ready), which are
/// identified by the `Header.state`.
union FutOut<F, T> {
    future: ManuallyDrop<F>,
    output: ManuallyDrop<T>,
}

/// Full task, combine of Header and Body.
///
/// Refered by Task, Wakers and JoinHandle. So the RawTask can only
/// be dropped when not refered:
///   - Task: Header.state == TaskState::Closed
///   - Wakers: Header.waker_refs == 0
///   - JoinHandle: Header.detached == true
#[repr(C)]
struct RawTask<F, T> {
    header: Header,
    u: FutOut<F, T>,
}

impl<F, T> RawTask<F, T>
where F: Future<Output = T> + 'static
{
    fn from_future(future: F) -> Self {
        RawTask {
            header: Header {
                state: TaskState::Active,
                detached: false,
                waker_refs: 0,
                awaker: None,

                vtable: &RawTaskVtable {
                    poll_task: Self::poll_task,
                },
            },
            u: FutOut {
                future: ManuallyDrop::new(future)
            },
        }
    }

    fn poll_task(ptr: *mut Header) {
        let header = unsafe { &mut *ptr };
        let raw_task = unsafe { &mut *(ptr as *mut RawTask<F, T>) };

        // TODO check other state
        if header.state != TaskState::Active {
            panic!("state: {:p} {:?}", header, header.state);
        }
        header.state = TaskState::Running;

        // new waker
        let raw_waker = header.create_raw_waker();
        let waker = unsafe { Waker::from_raw(raw_waker) };
        let mut cx = Context::from_waker(&waker);

        // SAFETY: header.state is Running, so `u` is `future`
        let future: &mut F = unsafe { &mut *raw_task.u.future };

        // SAFETY: we make sure that the task (who contains the future)
        // will not be moved.
        let pinned = unsafe { Pin::new_unchecked(future) };

        match pinned.poll(&mut cx) {
            Poll::Pending => {
                // the poll() may change the state from `Running`
                if header.state == TaskState::Running {
                    header.state = TaskState::Pending;
                }
            }
            Poll::Ready(output) => {
                if header.detached {
                    // there is no JoinHandle waiting, so drop self
                    header.state = TaskState::Closed;
                    header.try_drop();

                } else {
                    // drop the future before saving the output
                    // SAFETY: the `u.future` will never be used later
                    unsafe { drop_in_place(&mut *raw_task.u.future); }

                    // save the output for the JoinHandle to read
                    raw_task.u.output = ManuallyDrop::new(output);
                    header.state = TaskState::Ready;

                    // wake up the JoinHandle if it has been polled
                    if let Some(waker) = header.awaker.take() {
                        waker.wake();
                    }
                }
            }
        }
    }
}

impl Header {
    fn try_drop(&mut self) {
        if self.state == TaskState::Closed && self.waker_refs == 0 {
            unsafe { drop(Box::from_raw(self)); }
        }
    }

    const RAW_WAKER_VTABLE: RawWakerVTable = RawWakerVTable::new(
        Self::waker_clone,
        Self::waker_wake,
        Self::waker_wake_by_ref,
        Self::waker_drop,
    );

    fn create_raw_waker(&mut self) -> RawWaker {
        self.waker_refs += 1;
        RawWaker::new(self as *const Header as *const (), &Self::RAW_WAKER_VTABLE)
    }

    unsafe fn waker_clone(ptr: *const ()) -> RawWaker {
        let ptr = ptr as *mut Header;
        let header = unsafe { &mut *ptr };
        header.create_raw_waker()
    }
    unsafe fn waker_wake(ptr: *const ()) {
        Self::waker_wake_by_ref(ptr);
        Self::waker_drop(ptr); // consume the waker
    }
    unsafe fn waker_wake_by_ref(ptr: *const ()) {
        let ptr = ptr as *mut Header;
        let header = unsafe { &mut *ptr };
        match &header.state {
            TaskState::Active => (),
            TaskState::Pending | TaskState::Running => {
                header.state = TaskState::Active;
                Task::active(header);
            }
            state => {
                todo!("unexpected state: {:?}", state);
            }
        }
    }
    unsafe fn waker_drop(ptr: *const ()) {
        let ptr = ptr as *mut Header;
        let header = unsafe { &mut *ptr };
        header.waker_refs -= 1;
        header.try_drop();
    }
}

// JoinHandle
pub struct JoinHandle<T>(*mut RawTask<std::future::Pending<T>, T>);

impl<T> JoinHandle<T> {
    fn from_task_header(ptr: *mut Header) -> Self {
        let ptr = ptr as *mut RawTask<std::future::Pending<T>, T>;
        JoinHandle(ptr)
    }

    fn try_get_output(&mut self) -> Option<T> {
        let raw_task = unsafe { &mut *self.0 };
        if raw_task.header.state == TaskState::Ready {
            raw_task.header.state = TaskState::Closed;
            Some(unsafe { ManuallyDrop::take(&mut raw_task.u.output) })
        } else { // TODO Ok for Active and Pending, but what for others?
            None
        }
    }
}

impl<T> Future for JoinHandle<T> {
    type Output = T;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let join_handle = self.get_mut();

        match join_handle.try_get_output() {
            Some(output) => Poll::Ready(output),
            None => {
                let raw_task = unsafe { &mut *join_handle.0 };
                raw_task.header.awaker = Some(cx.waker().clone());
                Poll::Pending
            }
        }
    }
}

impl<T> Drop for JoinHandle<T> {
    fn drop(&mut self) {
        let raw_task = unsafe { &mut *self.0 };
        let header = &mut raw_task.header;

        match header.state {
            TaskState::Closed => (),
            TaskState::Ready => {
                unsafe { drop_in_place(&mut *raw_task.u.output); }
            }
            _ => {
                // the task is still alive
                header.detached = true;
                return;
            }
        }

        header.try_drop();
    }
}

// Task
struct Task(*mut Header);

impl Task {
    thread_local! {
        static ACTIVE_TASKS: RefCell<VecDeque<Task>> = RefCell::new(VecDeque::new());
    }

    fn active(header: *mut Header) {
        Self::ACTIVE_TASKS.with_borrow_mut(
            |active_tasks| active_tasks.push_back(Task(header)));
    }

    fn run<T>(mut outer_join: JoinHandle<T>) -> T {
        loop {
            loop {
                let mut active_tasks = Self::ACTIVE_TASKS.take();
                if active_tasks.is_empty() {
                    break;
                }
                println!("looooop: ready={}", active_tasks.len());

                while let Some(task) = active_tasks.pop_front() {
                    let header = unsafe { &mut *task.0 };
                    println!(">>> scheduler task: {:p} {:?}", header, header.state);
                    (header.vtable.poll_task)(header);
                }
            }

            if let Some(output) = outer_join.try_get_output() {
                println!("runtime end");
                return output;
            }

            event::run();
        }
    }
}

pub fn spwan<F, T>(future: F) -> JoinHandle<T>
where F: Future<Output = T> + 'static
{
    let raw_task = Box::leak(Box::new(RawTask::from_future(future)));

    let header = &mut raw_task.header;
    println!("  task: {:p}", header);

    Task::active(header);

    JoinHandle::from_task_header(header)
}

pub fn block_on<F, T>(future: F) -> T
where F: Future<Output = T> + 'static
{
    Task::run(spwan(future))
}
