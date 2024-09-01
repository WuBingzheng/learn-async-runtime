use std::cell::OnceCell;
use std::os::fd::{RawFd};
use std::task::Waker;
use polling::{Event, Events, Poller, PollMode};
use std::time::Duration;
use std::os::fd::AsFd;

thread_local! {
    static POLLER: OnceCell<Poller> = OnceCell::new();
}

pub fn run(timeout: Option<Duration>) {
    POLLER.with(|poller| {
        let mut events = Events::new();
        // events.clear(); TODO
        poller.get().unwrap().wait(&mut events, timeout).unwrap();

        for ev in events.iter() {
            let waker = unsafe { &mut *(ev.key as *mut Waker) };
            waker.wake_by_ref();
            println!("get event: {}", ev.key);
        }
    });
}

pub fn add_readable(raw_fd: RawFd, waker: *mut Waker) {
    POLLER.with(|poller| {
        let event = Event::readable(waker as usize);
        unsafe { poller.get().unwrap().add_with_mode(raw_fd, event, PollMode::Edge).unwrap() }
    });
}

pub fn add_writable(raw_fd: RawFd, waker: *mut Waker) {
    POLLER.with(|poller| {
        let event = Event::writable(waker as usize);
        unsafe { poller.get().unwrap().add_with_mode(raw_fd, event, PollMode::Edge).unwrap() }
    });
}

pub fn modify(raw_fd: impl AsFd, waker: *mut Waker) {
    POLLER.with(|poller| {
        let event = Event::all(waker as usize);
        poller.get().unwrap().modify_with_mode(raw_fd, event, PollMode::Edge).unwrap();
    });
}


pub fn init() {
    POLLER.with(|poller|
        poller.set(Poller::new().unwrap()).unwrap()
    );
}
