use std::cell::OnceCell;
use std::os::fd::{RawFd};
use std::task::Waker;
use polling::{Event, Events, Poller};

thread_local! {
    static POLLER: OnceCell<Poller> = OnceCell::new();
}

pub fn run() {
    POLLER.with(|poller| {
        let mut events = Events::new();
        // events.clear(); TODO
        poller.get().unwrap().wait(&mut events, None).unwrap();

        for ev in events.iter() {
            let waker = unsafe { &mut *(ev.key as *mut Option<Waker>) };
            let waker = waker.take().unwrap();
            waker.wake();
            println!("get event: {}", ev.key);
        }
    });
}

pub fn add_readable(raw_fd: RawFd, waker: &mut Option<Waker>) {
    POLLER.with(|poller| {
        let event = Event::readable(waker as *mut Option<Waker> as usize);
        unsafe { poller.get().unwrap().add(raw_fd, event).unwrap() }
    });
}

pub fn add_writable(raw_fd: RawFd, waker: &mut Option<Waker>) {
    POLLER.with(|poller| {
        let event = Event::writable(waker as *mut Option<Waker> as usize);
        unsafe { poller.get().unwrap().add(raw_fd, event).unwrap() }
    });
}

pub fn init() {
    POLLER.with(|poller|
        poller.set(Poller::new().unwrap()).unwrap()
    );
}
