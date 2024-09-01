use std::collections::BinaryHeap;
use std::pin::Pin;
use std::task::{Context, Waker, Poll};
use std::future::Future;
use std::sync::{Mutex};
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

// SleepFuture
struct SleepFuture {
    wake_time: SystemTime,
}

impl Future for SleepFuture {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let sleep_fut = self.get_mut();

        println!("poll sleep: {:?} {:?}", SystemTime::now(), sleep_fut.wake_time);
        if SystemTime::now() >= sleep_fut.wake_time {
            Poll::Ready(())
        } else {
            let mut timer = TIMER.lock().unwrap();
            let waker = cx.waker().clone();
            timer.push(SleepWaker{ wake_time:sleep_fut.wake_time, waker});

            Poll::Pending
        }
    }
}

pub fn sleep_inner(d: Duration) -> impl Future<Output = ()> {
    SleepFuture {
        wake_time: SystemTime::now() + d,
    }
}

pub fn timer_poll() -> Option<Duration> {
    let now = SystemTime::now();
    let mut timer = TIMER.lock().unwrap();
    while let Some(sleep_waker) = timer.peek() {
        if sleep_waker.wake_time <= now {
            println!("runtime: wake {:?}", sleep_waker.wake_time);
            let sleep_waker = timer.pop().unwrap();
            sleep_waker.waker.wake();
        } else {
            return Some(sleep_waker.wake_time.duration_since(now).unwrap());
        }
    }
    None
}
