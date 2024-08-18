use std::cell::OnceCell;
use std::cell::RefCell;
use std::os::fd::{RawFd};
use std::task::Waker;
use std::collections::BTreeMap;
use std::time::{Instant, Duration};
use polling::{Event, Events, Poller};

thread_local! {
    static POLLER: OnceCell<Poller> = OnceCell::new();
    static TIMER_SEQ: RefCell<usize> = RefCell::new(0);
    static TIMERS: RefCell<BTreeMap<Timer, Waker>> = RefCell::new(BTreeMap::new());
}

pub fn run_event(timeout: Option<Duration>) {
    POLLER.with(|poller| {
        let mut events = Events::new();
        // events.clear(); TODO
        poller.get().unwrap().wait(&mut events, timeout).unwrap();

        for ev in events.iter() {
            let waker = unsafe { &mut *(ev.key as *mut Option<Waker>) };
            let waker = waker.take().unwrap();
            waker.wake();
            println!("get event: {}", ev.key);
        }
    });
}

pub fn run_timer() -> Option<Duration> {
    TIMERS.with_borrow(|timers| println!("timer count in run_timer: {}", timers.len()));

    TIMERS.with_borrow_mut(|timers| {
        while let Some((timer, _)) = timers.first_key_value() {
            let now = Instant::now();
            if timer.at > now {
                return Some(timer.at - now);
            }
            println!("timeout!!! {:?}", timer.at);
            let (_, waker) = timers.pop_first().unwrap();
            waker.wake();
        }
        None
    })
}

pub fn add_readable(raw_fd: RawFd, waker: &mut Option<Waker>) {
    POLLER.with(|poller| {
        let event = Event::readable(waker as *mut Option<Waker> as usize);
        //unsafe { poller.get().unwrap().add(raw_fd, event).unwrap() }
        let _  = unsafe { poller.get().unwrap().add(raw_fd, event) };
    });
}

pub fn add_writable(raw_fd: RawFd, waker: &mut Option<Waker>) {
    POLLER.with(|poller| {
        let event = Event::writable(waker as *mut Option<Waker> as usize);
        unsafe { poller.get().unwrap().add(raw_fd, event).unwrap() }
    });
}

#[derive(PartialOrd, Ord, PartialEq, Eq)]
struct Timer {
    at: Instant,
    seq: usize,
}

pub fn add_timer(at: Instant, waker: Waker) {
    let seq = TIMER_SEQ.with_borrow_mut(|seq| {
        *seq += 1;
        *seq
    });
    TIMERS.with_borrow_mut(|timers|
        timers.insert(Timer { at, seq}, waker)
    );
    TIMERS.with_borrow(|timers| println!("timer count after add_timer: {}", timers.len()));
}

pub fn init() {
    POLLER.with(|poller|
        poller.set(Poller::new().unwrap()).unwrap()
    );

    TIMERS.set(BTreeMap::new());
}
