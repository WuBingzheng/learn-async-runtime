#![feature(noop_waker)]

use std::collections::{BinaryHeap, HashMap, VecDeque};
use std::pin::Pin;
use std::task::{Context, Waker, Poll, Wake};
use std::future::Future;
use std::sync::{Mutex, Arc};
use std::cmp::Ordering;
use std::rc::Rc;
use std::fmt::Debug;
use std::time::{SystemTime, Duration};
use std::cell::RefCell;
use std::sync::atomic::{self, AtomicBool};


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


// SleepFuture
struct SleepFuture {
    wake_time: SystemTime,
    poll_cnt: usize,
}

impl Future for SleepFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let sleep_fut = self.get_mut();

        println!("poll sleep: {:?} {:?}", SystemTime::now(), sleep_fut.wake_time);
        if SystemTime::now() >= sleep_fut.wake_time {
            Poll::Ready(())
        } else {
            sleep_fut.poll_cnt += 1;

            let mut timer = TIMER.lock().unwrap();
            let waker = cx.waker().clone();
            timer.push(SleepWaker{ wake_time:sleep_fut.wake_time, waker});

            Poll::Pending
        }
    }
}

fn sleep_inner(d: Duration) -> impl Future<Output = ()> {
    SleepFuture {
        wake_time: SystemTime::now() + d,
        poll_cnt: 0,
    }
}

async fn sleep(d: Duration) -> usize {
    sleep_inner(d).await;
    0
}

// Channel

// Inner
struct Inner<T> {
    buffer: VecDeque<T>,
    capacity: usize,

    tx_cnt: usize,
    rx_cnt: usize,

    pending_tx: VecDeque<Waker>,
    pending_rx: VecDeque<Waker>,
}

// ChTx
struct ChTx<T> {
    inner: Rc<RefCell<Inner<T>>>,
}
impl<T> ChTx<T>
where T: Clone
{
    fn send(&self, item: T) -> impl Future<Output = ()> {
        ChTxSendFuture {
            inner: self.inner.clone(),
            item,
        }
    }
}

impl<T> Clone for ChTx<T> {
    fn clone(&self) -> Self {
        let inner = &self.inner;
        inner.borrow_mut().tx_cnt += 1;
        ChTx { inner: inner.clone() }
    }
}

impl<T> Drop for ChTx<T> {
    fn drop(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.tx_cnt -= 1;
        if inner.tx_cnt == 0 {
            inner.pending_rx.drain(..).for_each(|w| w.wake());
        }
    }
}

// ChTxSendFuture
struct ChTxSendFuture<T: Clone> {
    inner: Rc<RefCell<Inner<T>>>,
    item: T,
}
impl<T> Future for ChTxSendFuture<T>
where T: Clone
{
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.inner.borrow_mut();

        if inner.rx_cnt == 0 {
            Poll::Ready(())

        } else if inner.capacity == inner.buffer.len() {
            println!("send pending");

            let waker = cx.waker().clone();
            inner.pending_tx.push_back(waker);

            Poll::Pending

        } else {
            inner.buffer.push_back(self.item.clone());
            println!("send done");

            if let Some(rx_waker) = inner.pending_rx.pop_front() {
                println!("send wakes rx");
                rx_waker.wake();
            }

            Poll::Ready(())
        }
    }
}

struct ChRx<T: Debug> {
    inner: Rc<RefCell<Inner<T>>>,
}
impl<T> ChRx<T>
where T: Debug
{
    fn recv(&self) -> impl Future<Output = Option<T>> {
        ChRxRecvFuture {
            inner: self.inner.clone(),
        }
    }
}
impl<T> Clone for ChRx<T>
where T: Debug
{
    fn clone(&self) -> Self {
        let inner = &self.inner;
        inner.borrow_mut().rx_cnt += 1;
        ChRx { inner: inner.clone() }
    }
}

impl<T> Drop for ChRx<T>
where T: Debug
{
    fn drop(&mut self) {
        let mut inner = self.inner.borrow_mut();
        inner.rx_cnt -= 1;
        if inner.rx_cnt == 0 {
            inner.pending_tx.drain(..).for_each(|w| w.wake());
        }
    }
}


// ChRxRecvFuture
struct ChRxRecvFuture<T> {
    inner: Rc<RefCell<Inner<T>>>,
}
impl<T> Future for ChRxRecvFuture<T>
where T: Debug
{
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let mut inner = self.inner.borrow_mut();

        match inner.buffer.pop_front() {
            Some(item) => {
                println!("recv: {:?}", item);

                if let Some(tx_waker) = inner.pending_tx.pop_front() {
                    println!("recv wakes tx");
                    tx_waker.wake();
                }

                Poll::Ready(Some(item))
            }
            None => {

                if inner.tx_cnt == 0 {
                    Poll::Ready(None)

                } else {
                    println!("recv pending");
                    let waker = cx.waker().clone();
                    inner.pending_rx.push_back(waker);

                    Poll::Pending
                }
            }
        }
    }
}

fn new_channel<T: Debug>(capacity: usize) -> (ChTx<T>, ChRx<T>) {
    let ch = Inner {
        buffer: VecDeque::with_capacity(capacity),
        capacity,

        tx_cnt: 1,
        rx_cnt: 1,

        pending_rx: VecDeque::new(),
        pending_tx: VecDeque::new(),
    };

    let ch = Rc::new(RefCell::new(ch));
    let ch2 = ch.clone();
    (ChTx { inner: ch}, ChRx { inner: ch2 })
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
                    Poll::Ready(n) => {
                        self.all_tasks.remove(&id);
                        println!("runtime: task {:?} done {n}", id);
                    }
                }
            }

            let now = SystemTime::now();
            let mut timer = TIMER.lock().unwrap();
            while let Some(sleep_waker) = timer.peek() {
                if sleep_waker.wake_time <= now {
                    println!("runtime: wake {:?}", sleep_waker.wake_time);
                    let sleep_waker = timer.pop().unwrap();
                    sleep_waker.waker.wake();
                } else {
                    if !NEW_READY.load(atomic::Ordering::Relaxed) {
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

async fn send_5<T: Clone>(tx: ChTx<T>, items: &[T]) -> usize {
    for i in items {
        tx.send(i.clone()).await;
        println!(" --- send");
    }
    0
}
async fn recv_5<T: Debug>(tx: ChRx<T>) -> usize {
    for _ in 0..5 {
        match tx.recv().await {
            Some(n) => println!(" --- recv {n:?}"),
            None => {
                println!("recv finish!!!");
                break;
            }
        }
    }
    0
}

fn main() {
    let mut rt = Runtime::new();
    rt.spwan(r42());
    rt.spwan(sleep(Duration::from_secs(5)));
    rt.spwan(async { sleep_inner(Duration::from_secs(2)).await; 0 });

    let (tx, rx) = new_channel(1);
    rt.spwan(send_5(tx, &[1,2,3,4,5]));
    rt.spwan(recv_5(rx.clone()));
    rt.spwan(recv_5(rx.clone()));
    rt.spwan(recv_5(rx));

    rt.run();
}
