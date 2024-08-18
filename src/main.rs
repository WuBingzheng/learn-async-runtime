use std::future::Future;
use std::task::{Context, Poll};
use std::pin::Pin;

use std::time::Duration;

mod runtime;
mod event;
mod tcp;
mod timer;

async fn r42() -> usize {
    42
}
async fn r100() -> usize {
    let a = [1,2,3,4];
    let r = &a;
    let n = r42().await;

    let a2 = [11,12,13,14];
    let r2 = &a2;
    let n2 = r42().await;

    r[2] + r2[2] + n + n2
}
async fn hello() -> String {
    String::from("hello, world")
}

struct PendingOnce {
    get: bool,
}

impl Future for PendingOnce {
    type Output = usize;

    fn poll(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        let fut = self.get_mut();
        if !fut.get {
            fut.get = true;
            cx.waker().wake_by_ref();
            Poll::Pending
        } else {
            Poll::Ready(123)
        }
    }
}

async fn biz_woker() -> &'static str {
    println!("new: r42");
    let _r42 = runtime::spwan(r42());

    println!("new: r100");
    let _r100 = runtime::spwan(r100());

    println!("new: hello");
    let hello = runtime::spwan(hello());

    println!("new: pending-once");
    let r123 = runtime::spwan(PendingOnce{get: false});

    println!("new: joinhandles");
    runtime::spwan(async { println!("> ret: {}", _r42.await) });
    runtime::spwan(async { println!("> ret: {}", _r100.await) });
    runtime::spwan(async { println!("> ret: {}", hello.await) });
    runtime::spwan(async { println!("> ret: {}", r123.await) });

    let listen = tcp::TcpListener::bind("127.0.0.1:4444").await.unwrap();
    let mut stream = listen.accept().await.unwrap();

    let mut buf = [0u8; 100];
    let len = timer::timeout(Duration::from_secs(3), stream.read(&mut buf)).await;
    println!("read: {:?}: {}", len, std::str::from_utf8(&buf).unwrap());

    stream.write(&buf).await.unwrap();

    let olen = timer::timeout(Duration::from_secs(3), stream.read(&mut buf)).await;
    println!("read again: {:?}", olen);

    timer::sleep(Duration::from_secs(3)).await;
    "ok, done!"
}

fn main()
{
    event::init();

    let done = runtime::block_on(biz_woker());
    println!("{}", done);
}
