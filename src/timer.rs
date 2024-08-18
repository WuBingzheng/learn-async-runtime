use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;
use std::time::{Instant, Duration};
use std::marker::PhantomData;

use crate::event;

pub struct Timer {
    at: Instant,
}

impl Future for Timer {
    type Output = ();

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        if fut.at < Instant::now() {
            Poll::Ready(())
        } else {
            let waker = cx.waker().clone();
            event::add_timer(fut.at, waker);
            Poll::Pending
        }
    }
}

pub async fn sleep(d: Duration) {
    Timer {
        at: Instant::now() + d,
    }.await
}

pub async fn timeout<F, T>(d: Duration, fut: F) -> Option<T>
where F: Future<Output = T>
{
    Timeout {
        timer: Timer { at: Instant::now() + d },
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
                // TODO drop timer
                //
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
