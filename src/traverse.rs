use crate::error;
use crate::options::Options;
use crossbeam::channel::{Receiver,Sender};
use std::thread;
use std::fs::{ReadDir, read_dir};

pub struct TraverseThread {
    thread: std::thread::JoinHandle<Result<(),error::E>>
}

pub type FreeThreadTx = Sender<Sender<Task>>;
pub type FreeThreadRx = Receiver<Sender<Task>>;

impl TraverseThread {
    fn new(opts: Options, free_thread_queue: (Sender<Sender<Task>>, Receiver<Sender<Task>>)) -> TraverseThread {
        let th = thread::spawn(move || -> Result<(), error::E> {
            let tq = crossbeam::channel::unbounded();
            free_thread_queue.0.send(tq.0.clone())?;

            let t = tq.1.recv();

            Ok(())
        }
        );

        TraverseThread {
            thread: th
        }
    }
}

pub struct ThreadList {
    free_thread_queue: (Sender<Sender<Task>>, Receiver<Sender<Task>>),
    threads: Vec<TraverseThread>,
}

impl ThreadList {
    fn new(opts: Options) -> ThreadList {
        let mut v = Vec::new();
        let free_thread_queue = crossbeam::channel::unbounded();

        for i in 0..opts.num_threads {
            v.push( TraverseThread::new(opts.clone(), free_thread_queue.clone()) );
        }

        ThreadList {
            free_thread_queue,
            threads: v
        }
    }

    fn try_pop_free_thread(&self) -> Option<Sender<Task>> {
        let v = self.free_thread_queue.1.try_recv();
        match v {
            Ok(v) => Some(v),
            Err(_) => None
        }
    }

    fn pop_free_thread(&self) -> Result<Sender<Task>,error::E> {
        let e = self.free_thread_queue.1.recv()?;
        Ok(e)
    }
}

pub struct Traverser {
    pub opt: Options,
}

type TaskStack = Vec<Task>;

pub enum Task {
    ReadDir {},
}

pub fn traverse(t: &mut Traverser) -> Result<(), error::E> {
    let tl = ThreadList::new(t.opt.clone());
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() {
        let tl = ThreadList::new(2);
        let f1 = tl.pop_free_thread();
        let f2 = tl.pop_free_thread();

        std::thread::sleep( std::time::Duration::from_millis(10));
        let f3 = tl.try_pop_free_thread();

        assert!(f3.is_none());
    }
}
