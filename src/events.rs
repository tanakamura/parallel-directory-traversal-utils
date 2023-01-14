use std::ops::DerefMut;
use std::sync::{Arc, Mutex};

#[derive(Debug)]
pub struct DepChainV {
    pub completed: bool,
    pub waiter: Option<(
        crossbeam_channel::Sender<()>,
        crossbeam_channel::Receiver<()>,
    )>, // to notify all complete
}

impl DepChainV {
    pub fn get_channel_locked(
        &mut self,
    ) -> (
        crossbeam_channel::Sender<()>,
        crossbeam_channel::Receiver<()>,
    ) {
        if let Some(w) = &self.waiter {
            return w.clone();
        }

        let ret = crossbeam_channel::bounded(0);
        self.waiter = Some(ret.clone());

        return ret;
    }
}

#[derive(Clone, Debug)]
pub struct DepChain {
    pub v: Arc<Mutex<DepChainV>>,
    pub pred: Option<Box<DepChain>>, // None for first chain
}

impl DepChain {
    pub fn get_wait_channel(&mut self) -> Option<crossbeam_channel::Receiver<()>> {
        let mut v = self.v.lock().unwrap();
        if v.completed {
            return None;
        }

        Some(v.get_channel_locked().1)
    }

    pub fn wait(&self) {
        let mut v = self.v.lock().unwrap();

        if v.completed {
            return;
        }

        let rx = v.get_channel_locked().1;
        drop(v);

        rx.recv().unwrap();
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
            w.0.send(()).unwrap();
        }
    }
}
