use std::collections::{HashMap, VecDeque};
use std::pin::Pin;
use std::task::{Context, Waker, Poll, Wake};
use std::future::Future;
use std::sync::{Mutex, Arc};
use std::rc::Rc;
use std::fmt::Debug;
use std::time::Duration;
use std::cell::RefCell;
use std::sync::atomic::{self, AtomicBool};

mod channel;
mod sleep;


static READY_TASKS: Mutex<VecDeque<TaskId>> = Mutex::new(VecDeque::new());

static NEW_READY: AtomicBool = AtomicBool::new(false);

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

        NEW_READY.store(true, atomic::Ordering::Relaxed);
    }
}
async fn sleep(d: Duration) {
    sleep::sleep_inner(d).await;
}

// JoinHandle
struct JoinHandleInner {
    finished: bool,
    waker: Option<Waker>,
}

#[derive(Clone)]
struct JoinHandle(Rc<RefCell<JoinHandleInner>>);

impl Future for JoinHandle {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        if self.0.borrow().finished {
            Poll::Ready(())
        } else {
            let mut inner = self.0.borrow_mut();
            inner.waker = Some(cx.waker().clone());
            Poll::Pending
        }
    }
}


// Task
#[derive(Copy, Clone, PartialEq, Eq, Hash, Default, Debug)]
struct TaskId(usize);
struct Task {
    future: Pin<Box<dyn Future<Output = ()>>>,
    join_handle: JoinHandle,
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

    fn spwan(&mut self, future: impl Future<Output = ()> + 'static) -> JoinHandle { // why 'static
        println!("size of futures: {}", std::mem::size_of_val(&future));
        let join_inner = JoinHandleInner { finished: false, waker: None };
        let join_handle = JoinHandle(Rc::new(RefCell::new(join_inner)));

        let task = Task { future: Box::pin(future), join_handle: join_handle.clone() };

        let id = TaskId(self.next_id);
        self.next_id += 1;

        self.all_tasks.insert(id, task);

        //self.ready_tasks.push_back(id);
        let mut ready_tasks = READY_TASKS.lock().unwrap();
        ready_tasks.push_back(id);

        join_handle
    }

    fn run(&mut self) {
        while !self.all_tasks.is_empty() {
            NEW_READY.store(false, atomic::Ordering::Relaxed);

            let mut ready_tasks = std::mem::take(&mut *READY_TASKS.lock().unwrap());
            println!("looooop: all={} ready={}", self.all_tasks.len(), ready_tasks.len());

            while let Some(id) = ready_tasks.pop_front() {
                let mywaker = MyWaker { id };
                let waker = Arc::new(mywaker).into();
                let mut cx = Context::from_waker(&waker);

                let future = self.all_tasks.get_mut(&id).unwrap().future.as_mut();
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
            }

            if let Some(next_time) = sleep::timer_poll() {
                if !NEW_READY.load(atomic::Ordering::Relaxed) {
                    println!("runtime: sleep: {:?}", next_time);
                    std::thread::sleep(next_time);
                }
            }
        }
        println!("runtime end");
    }
}

async fn r42() -> usize {
    42
}
async fn noret_r42(x42: &mut usize) {
    *x42 = r42().await;
}

async fn send_5<T: Clone>(tx: channel::ChTx<T>, items: &[T]) {
    for i in items {
        tx.send(i.clone()).await;
        println!(" --- send");
    }
}
async fn recv_5<T: Debug>(tx: channel::ChRx<T>) {
    for _ in 0..5 {
        match tx.recv().await {
            Some(n) => println!(" --- recv {n:?}"),
            None => {
                println!("recv finish!!!");
                break;
            }
        }
    }
}

fn main() {
    let mut rt = Runtime::new();

    //let r42 = rt.spwan(r42());
    rt.spwan(sleep(Duration::from_secs(5)));
    rt.spwan(sleep::sleep_inner(Duration::from_secs(2)));

    let (tx, rx) = channel::new_channel(1);
    rt.spwan(send_5(tx, &[1,2,3,4,5]));
    rt.spwan(recv_5(rx.clone()));
    rt.spwan(recv_5(rx.clone()));
    rt.spwan(recv_5(rx));

    //rt.spwan(async { println!("============>>>> ret: {}", r42.await); 0 });

    rt.run();
}
