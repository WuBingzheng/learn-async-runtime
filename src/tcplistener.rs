use std::rc::Rc;
use std::net::{ToSocketAddrs, TcpStream};
use std::io::{Result, ErrorKind};
use std::future::Future;
use std::task::{Context, Waker, Poll};
use std::pin::Pin;
use std::os::fd::AsRawFd;

use crate::event;

pub struct TcpListener {
    inner: Rc<std::net::TcpListener>,
}

impl TcpListener {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> Result<TcpListener> {
        let inner = std::net::TcpListener::bind(addr)?;
        inner.set_nonblocking(true).unwrap();
        Ok(TcpListener { inner: Rc::new(inner) })
    }
    pub async fn accept(&self) -> Result<TcpStream> {
        let fut = AcceptFut {
            inner: self.inner.clone(),
            waker: None,
        };
        fut.await
    }
}

struct AcceptFut {
    inner: Rc<std::net::TcpListener>,
    waker: Option<Waker>,
}

impl Future for AcceptFut {
    type Output = Result<TcpStream>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        match fut.inner.accept() {
            Ok((socket, addr)) => {
                println!("accept: {:?}", addr);
                Poll::Ready(Ok(socket))
            }
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock {
                    let waker = cx.waker().clone();
                    fut.waker = Some(waker);
                    event::add_readable(fut.inner.as_raw_fd(), &mut fut.waker);
                    /*
                    let event = Event::readable(&listener.waker as *const Cell<Option<Waker>> as usize);
                    POLLER.with(
                        |poller| unsafe { poller.get().unwrap().add(listener.inner.as_raw_fd(), event).unwrap() }
                    );
                    */
                    Poll::Pending
                } else {
                    Poll::Ready(Err(e))
                }
            }
        }
    }
}
