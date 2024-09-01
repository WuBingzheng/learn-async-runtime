use std::collections::{VecDeque};
use std::pin::Pin;
use std::task::{Context, Waker, Poll};
use std::future::Future;
use std::rc::Rc;
use std::fmt::Debug;
use std::cell::RefCell;


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
pub struct ChTx<T> {
    inner: Rc<RefCell<Inner<T>>>,
}
impl<T> ChTx<T>
where T: Clone
{
    pub fn send(&self, item: T) -> impl Future<Output = ()> {
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
pub struct ChTxSendFuture<T: Clone> {
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

pub struct ChRx<T: Debug> {
    inner: Rc<RefCell<Inner<T>>>,
}
impl<T> ChRx<T>
where T: Debug
{
    pub fn recv(&self) -> impl Future<Output = Option<T>> {
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

pub fn new_channel<T: Debug>(capacity: usize) -> (ChTx<T>, ChRx<T>) {
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
