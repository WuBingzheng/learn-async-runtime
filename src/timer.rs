use std::cell::RefCell;
use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;
use std::time::{Instant, Duration};
use std::marker::PhantomData;
use std::collections::BTreeMap;
use std::task::Waker;

thread_local! {
    static TIMERS: RefCell<BTreeMap<Timer, Waker>> = RefCell::new(BTreeMap::new());
    static TIMER_SEQ: RefCell<usize> = RefCell::new(0);
}

pub fn run() -> Option<Duration> {
    TIMERS.with_borrow_mut(|timers| {
        while let Some((timer, _)) = timers.first_key_value() {
            let now = Instant::now();
            if timer.at > now {
                return Some(timer.at - now);
            }
            println!("timeout!!! {:?}", timer.at);
            let (mut timer, waker) = timers.pop_first().unwrap();
            timer.seq = 0; // to avoid the delete_timer() in Drop
            waker.wake()
        }
        None
    })
}

fn next_seq() -> usize {
    TIMER_SEQ.with_borrow_mut(|seq| {
        *seq += 1;
        *seq
    })
}

fn add_timer(timer: Timer, waker: Waker) {
    TIMERS.with_borrow_mut(|timers|
        timers.insert(timer, waker)
    );
}
fn delete_timer(timer: &Timer) {
    TIMERS.with_borrow_mut(|timers|
        timers.remove(timer)
    );
}

#[derive(PartialOrd, Ord, PartialEq, Eq, Clone)]
pub struct Timer {
    at: Instant,
    seq: usize,
}

impl Timer {
    fn from_at(at: Instant) -> Self {
        Timer { at, seq: 0 }
    }
    fn from_duration(d: Duration) -> Self {
        Self::from_at(Instant::now() + d)
    }
}

impl Future for Timer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let timer = self.get_mut();
        if timer.at < Instant::now() {
            Poll::Ready(())
        } else {
            if timer.seq == 0 {
                timer.seq = next_seq();

                let waker = cx.waker().clone();

                add_timer(timer.clone(), waker);
            }
            Poll::Pending
        }
    }
}

impl Drop for Timer {
    fn drop(&mut self) {
        if self.seq != 0 {
            self.seq = 0;
            delete_timer(self);
        }
    }
}

// sleep
pub async fn sleep(d: Duration) {
    Timer::from_duration(d).await
}

// timeout
pub async fn timeout<F, T>(d: Duration, fut: F) -> Option<T>
where F: Future<Output = T>
{
    Timeout {
        timer: Timer::from_duration(d),
        fut,
        phantom: PhantomData::<T>,
    }.await
}

pub struct Timeout<F, T> {
    timer: Timer,
    fut: F,
    phantom: PhantomData<T>,
}

impl<F, T> Future for Timeout<F, T>
where F: Future<Output = T>
{
    type Output = Option<T>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = unsafe { self.get_unchecked_mut() }; // TODO is this unsafe ok?

        let pinned = unsafe { Pin::new_unchecked(&mut fut.fut) };

        println!("poll timeout");
        match pinned.poll(cx) {
            Poll::Ready(output) => {
                Poll::Ready(Some(output))
            }
            Poll::Pending => {
                let pinned = unsafe { Pin::new_unchecked(&mut fut.timer) };
                match pinned.poll(cx) {
                    Poll::Ready(_) => Poll::Ready(None),
                    Poll::Pending => Poll::Pending,
                }
            }
        }
    }
}
