#![feature(noop_waker)]

use std::collections::VecDeque;
use std::pin::Pin;
use std::task::{Context, Waker, Poll};
use std::future::Future;

// Inner
struct Inner<T> {
    buffer: VecDeque<T>,
    capacity: usize,
}
impl<T> Inner<T> {
    fn send(&mut self, item: T) -> Option<T> {
        if self.capacity == self.buffer.len() {
            Some(item)
        } else {
            self.buffer.push_back(item);
            None
        }
    }
}

// ChTx
struct ChTx<T> {
    inner: Rc<RefCell<Inner<T>>>,
}
impl<T> ChTx<T> {
    async fn send(&self, item: T) -> usize {
        ChTxSendFuture {
            inner: self.inner.clone(),
            item,
        }
    }
}

// ChTxSendFuture
struct ChTxSendFuture<T> {
    inner: Rc<RefCell<Inner<T>>>,
    item: T,
}
impl<T> Future for ChTxSendFuture<T> {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<Self::Output> {
        let mut ch = self.inner.get_mut().unwrap();

        match ch.send(self.value.clone()) {
            Some(_) => {
                println!("send pending");
                Poll::Pending
            }
            None => {
                println!("send done");
                Poll::Ready(0)
            }
        }
    }
}

struct ChRx<T> {
    inner: Rc<RefCell<Inner<T>>>,
}

// Channel
impl<T> Channel<T> {
    fn new(capacity: usize) -> (ChTx<T>, ChRx<T>) {
        let ch = inner {
            buffer: VecDeque::with_capacity(capacity),
            capacity,
        };

        let ch = Rc::new(RefCell::new(ch));
        let ch2 = ch.clone();
        (ChTx { inner: ch}, ChRx { inner: ch2 })
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
                Poll::Pending => self.tasks.push_back(task),
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

async fn outer() -> usize {
    r42()
}

fn main() {
    let mut rt = Runtime::new();

    rt.spwan(r42());
    rt.run();
}
