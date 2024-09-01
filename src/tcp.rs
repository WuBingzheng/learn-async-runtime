use std::cell::UnsafeCell;
use std::io::{Read, Write};
use std::net::ToSocketAddrs;
use std::io::{Result, ErrorKind};
use std::future::Future;
use std::task::{Context, Waker, Poll};
use std::pin::Pin;
use std::os::fd::AsRawFd;

use socket2;

use crate::event;

pub struct TcpListener {
    inner: std::net::TcpListener,
    waker: UnsafeCell<Option<Waker>>,
}

impl TcpListener {
    pub async fn bind<A: ToSocketAddrs>(addr: A) -> Result<TcpListener> {
        let inner = std::net::TcpListener::bind(addr)?;
        inner.set_nonblocking(true)?;
        Ok(TcpListener { inner, waker: UnsafeCell::new(None) })
    }
    pub async fn accept(&self) -> Result<TcpStream> {
        AcceptFut { listener: self }.await
    }
}

struct AcceptFut<'a> {
    listener: &'a TcpListener,
}

impl<'a> Future for AcceptFut<'a> {
    type Output = Result<TcpStream>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        let inner = &fut.listener.inner;
        match inner.accept() {
            Ok((socket, addr)) => {
                println!("accept: {:?}", addr);
                Poll::Ready(Ok(TcpStream::from_std(socket)))
            }
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                let waker_cell = unsafe { &mut *fut.listener.waker.get() };
                if waker_cell.is_none() {
                    let waker = cx.waker().clone();
                    let waker = waker_cell.insert(waker);
                    event::add_readable(inner.as_raw_fd(), waker);
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

pub struct TcpStream {
    inner: std::net::TcpStream,
    readable_waker: Option<Waker>,
    writable_waker: Option<Waker>,
}

impl TcpStream {
    fn from_std(inner: std::net::TcpStream) -> Self {
        TcpStream {
            inner,
            readable_waker: None,
            writable_waker: None,
        }
    }

    pub async fn connect(addr: &socket2::SockAddr) -> Result<Self> {
        let sock = socket2::Socket::new(socket2::Domain::IPV4, socket2::Type::STREAM, None)?;

        sock.set_nonblocking(true)?;

        let r = sock.connect(addr);

        let mut stream = TcpStream::from_std(sock.into());

        match r {
            Ok(()) => Ok(stream),
            Err(e) if e.raw_os_error() == Some(libc::EINPROGRESS) => {
                ConnectFut { stream: &mut stream }.await?;
                Ok(stream)
            }
            Err(e) => Err(e),
        }
    }

    pub async fn read(&mut self, buf: &mut [u8]) -> Result<usize> {
        ReadFut {
            stream: self,
            buf,
        }.await
    }
    pub async fn write(&mut self, buf: &[u8]) -> Result<usize> {
        WriteFut {
            stream: self,
            buf,
        }.await
    }
}

struct ReadFut<'a> {
    stream: &'a mut TcpStream,
    buf: &'a mut [u8],
}

impl<'a> Future for ReadFut<'a> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        let stream = &mut fut.stream;
        let inner = &mut stream.inner;
        match inner.read(fut.buf) {
            Ok(len) => Poll::Ready(Ok(len)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if stream.readable_waker.is_none() {
                    let waker = cx.waker().clone();
                    let waker = stream.readable_waker.insert(waker);
                    event::add_readable(inner.as_raw_fd(), waker);
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

struct WriteFut<'a> {
    stream: &'a mut TcpStream,
    buf: &'a [u8],
}

impl<'a> Future for WriteFut<'a> {
    type Output = Result<usize>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        let stream = &mut fut.stream;
        let inner = &mut stream.inner;
        match inner.write(fut.buf) {
            Ok(len) => Poll::Ready(Ok(len)),
            Err(e) if e.kind() == ErrorKind::WouldBlock => {
                if stream.writable_waker.is_none() {
                    let waker = cx.waker().clone();
                    let waker = stream.writable_waker.insert(waker);
                    event::add_writable(inner.as_raw_fd(), waker);
                }
                Poll::Pending
            }
            Err(e) => Poll::Ready(Err(e)),
        }
    }
}

struct ConnectFut<'a> {
    stream: &'a mut TcpStream,
}

impl<'a> Future for ConnectFut<'a> {
    type Output = Result<()>;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        let stream = &mut fut.stream;
        let inner = &mut stream.inner;

        if stream.writable_waker.is_none() {
            let waker = cx.waker().clone();
            let waker = stream.writable_waker.insert(waker);
            event::add_writable(inner.as_raw_fd(), waker);
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
}
