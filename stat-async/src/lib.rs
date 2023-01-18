use std::future::Future;
use io_uring::IoUring;
use std::pin::Pin;

pub mod error;

type TaskOutput = Result<(),error::E>;

struct Task {
    f: Pin<Box<dyn Future<Output=TaskOutput>>>
}

impl Task {
    fn new(f : Pin<Box<dyn Future<Output=TaskOutput>>>) -> Task {
        Task { f }
    }
}

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


#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_works() {
        let mut s = Scheduler::new(16);
        s.spawn( async { Ok(()) } );
        s.wait_for_empty();
    }
}
