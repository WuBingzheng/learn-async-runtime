#![feature(noop_waker)]

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Waker, Poll};
use std::future::Future;

use std::time::{SystemTime, Duration};

// SleepFuture
struct SleepFuture {
    wake_time: SystemTime,
    poll_cnt: usize,
}

impl Future for SleepFuture {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        let sleep_fut = self.get_mut();

        //println!("poll sleep: {:?} {:?}", SystemTime::now(), sleep_fut.wake_time);
        if SystemTime::now() >= sleep_fut.wake_time {
            Poll::Ready(sleep_fut.poll_cnt)
        } else {
            sleep_fut.poll_cnt += 1;
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
struct Task {
    future: Pin<Box<dyn Future<Output = usize>>>,
}

// Runtime
struct Runtime {
    tasks: VecDeque<Task>,
}

impl Runtime {
    fn new() -> Self {
        Runtime {
            tasks: VecDeque::new(),
        }
    }

    fn spwan(&mut self, future: impl Future<Output = usize> + 'static) { // why 'static
        let task = Task { future: Box::pin(future) };
        self.tasks.push_back(task);
    }

    fn run(&mut self) {
        let mut cx = Context::from_waker(Waker::noop());

        while let Some(mut task) = self.tasks.pop_front() {
            let future = task.future.as_mut();
            match future.poll(&mut cx) {
                Poll::Pending => {
                    //println!("runtime: task pending");
                    self.tasks.push_back(task);
                }
                Poll::Ready(n) => {
                    println!("runtime: task done {n}");
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
    rt.spwan(sleep(Duration::from_secs(4)));
    rt.spwan(sleep(Duration::from_secs(2)));
    rt.run();
}
