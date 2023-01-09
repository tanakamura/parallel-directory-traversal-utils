use nix::sys::eventfd;
use std::sync::{Arc, Mutex};

pub struct DepChainV {
    complete: bool,
    waiter: Option<Arc<std::sync::Barrier>>, // to notify all complete
    complete_callbacks: Vec<Box<dyn FnOnce() + Send>>,
}

impl DepChainV {
    fn completed(&self) -> bool {
        self.complete
    }
}

#[derive(Clone)]
pub struct DepChain {
    pub v: Arc<Mutex<DepChainV>>,
}

impl DepChain {
    pub fn wait(&self) {
        let mut v = self.v.lock().unwrap();
        if v.completed() {
            return;
        }

        let b = Arc::new(std::sync::Barrier::new(2));
        v.waiter = Some(b.clone());
        drop(v);

        b.wait();
    }

    pub fn new() -> DepChain {
        let v = DepChainV {
            complete: false,
            waiter: None,
            complete_callbacks: Vec::new(),
        };

        DepChain {
            v: Arc::new(Mutex::new(v)),
        }
    }

    pub fn notify_complete(&self) {
        let mut v = self.v.lock().unwrap();
        v.complete = true;
        if let Some(b) = &v.waiter {
            // all finished
            b.wait();
        }

        while let Some(f) = v.complete_callbacks.pop() {
            f()
        }
    }

    pub fn add_complete_callback<F>(&self, f: F)
    where
        F: FnOnce() + Send + 'static,
    {
        let mut v = self.v.lock().unwrap();
        if v.complete {
            f();
        } else {
            v.complete_callbacks.push(Box::new(f));
        }
    }
}
