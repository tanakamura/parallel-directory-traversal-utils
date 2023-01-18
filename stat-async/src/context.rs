use io_uring::IoUring;
use std::future::Future;
use std::rc::Rc;
use std::task::{RawWaker, RawWakerVTable};

use crate::task::{Task,TaskOutput};

pub struct Scheduler {
    ring: IoUring,
    actives: Vec<Task>,
    wait_for_ring_avail: Vec<Task>
}

impl Scheduler {
    pub fn new(entries: u32) -> Scheduler {
        Scheduler {
            ring: IoUring::new(entries).unwrap(),
            actives: Vec::new(),
            wait_for_ring_avail: Vec::new()
        }
    }

    pub fn spawn<T>(&mut self, future: T)
    where
        T: Future<Output=TaskOutput> + 'static
    {
        let p = Box::pin(future);
        self.actives.push( Task::new(p) );
    }

    pub fn wait_for_empty(&mut self) {
    }
}

type SchedulerPtr = Rc<Scheduler>;

unsafe fn schedptr_clone (raw: *const ()) -> RawWaker {
    SchedulerPtr::increment_strong_count(raw as *const Scheduler);
    RawWaker::new(raw, &VTABLE)
}
unsafe fn schedptr_wake (raw: *const ()) {
}
unsafe fn schedptr_wake_by_ref (raw: *const ()) {
}
unsafe fn schedptr_drop (raw: *const ()) {
    SchedulerPtr::decrement_strong_count(raw as *const Scheduler)
}

static VTABLE: RawWakerVTable = RawWakerVTable::new(
    schedptr_clone,
    schedptr_wake,
    schedptr_wake_by_ref,
    schedptr_drop,
);
