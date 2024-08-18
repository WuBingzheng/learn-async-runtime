use std::rc::Rc;
use std::io::{Read, Write};
use std::cell::RefCell;
use std::net::ToSocketAddrs;
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
                Poll::Ready(Ok(TcpStream::from_std(socket)))
            }
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock {
                    let waker = cx.waker().clone();
                    fut.waker = Some(waker);
                    event::add_readable(fut.inner.as_raw_fd(), &mut fut.waker);
                    Poll::Pending
                } else {
                    Poll::Ready(Err(e))
                }
            }
        }
    }
}

pub struct TcpStream {
    inner: Rc<RefCell<std::net::TcpStream>>,
}

impl TcpStream {
    fn from_std(s: std::net::TcpStream) -> Self {
        TcpStream {
            inner: Rc::new(RefCell::new(s)),
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        ReadFut {
            inner: self.inner.clone(),
            buf: buf,
            waker: None,
        }.await
    }
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize> {
        WriteFut {
            inner: self.inner.clone(),
            buf: buf,
            waker: None,
        }.await
    }
}

struct ReadFut<'a> {
    inner: Rc<RefCell<std::net::TcpStream>>,
    buf: &'a mut [u8],
    waker: Option<Waker>,
}

impl<'a> Future for ReadFut<'a> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        let mut inner = fut.inner.borrow_mut();
        match inner.read(fut.buf) {
            Ok(len) => {
                Poll::Ready(Ok(len))
            }
            Err(e) => {
                if e.kind() != ErrorKind::WouldBlock {
                    Poll::Ready(Err(e))
                } else {
                    let waker = cx.waker().clone();
                    fut.waker = Some(waker);
                    event::add_readable(inner.as_raw_fd(), &mut fut.waker);
                    Poll::Pending
                }
            }
        }
    }
}

struct WriteFut<'a> {
    inner: Rc<RefCell<std::net::TcpStream>>,
    buf: &'a [u8],
    waker: Option<Waker>,
}

impl<'a> Future for WriteFut<'a> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        let mut inner = fut.inner.borrow_mut();
        match inner.write(fut.buf) {
            Ok(len) => {
                Poll::Ready(Ok(len))
            }
            Err(e) => {
                if e.kind() == ErrorKind::WouldBlock {
                    let waker = cx.waker().clone();
                    fut.waker = Some(waker);
                    event::add_writable(inner.as_raw_fd(), &mut fut.waker);
                    Poll::Pending
                } else {
                    Poll::Ready(Err(e))
                }
            }
        }
    }
}
