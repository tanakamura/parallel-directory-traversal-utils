use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct DepChainV {
    pub completed: bool,
    pub waiter: Option<Arc<std::sync::Barrier>>, // to notify all complete
}

#[derive(Clone, Debug)]
pub struct DepChain {
    pub v: Arc<Mutex<DepChainV>>,
    pub pred: Option<Box<DepChain>>, // None for first chain
}

impl DepChain {
    pub fn wait(&self) {
        let mut v = self.v.lock().unwrap();

        if v.completed {
            return;
        }

        let b = Arc::new(std::sync::Barrier::new(2));
        v.waiter = Some(b.clone());
        drop(v);

        b.wait();
    }

    pub fn is_completed(&self) -> bool {
        self.v.lock().unwrap().completed
    }

    pub fn new() -> DepChain {
        let v = DepChainV {
            completed: false,
            waiter: None,
        };

        DepChain {
            v: Arc::new(Mutex::new(v)),
            pred: None,
        }
    }

    pub fn complete(&mut self) {
        let mut v = self.v.lock().unwrap();
        let mut v = v.deref_mut();
        v.completed = true;

        let mut b = None;
        std::mem::swap(&mut b, &mut v.waiter);

        if let Some(w) = b {
            w.wait();
        }
    }
}
