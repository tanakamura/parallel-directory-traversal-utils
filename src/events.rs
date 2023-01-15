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
pub enum DepChain {
    Value {
        v: Arc<Mutex<DepChainV>>,
        pred: Option<Box<DepChain>>, // None for first chain
    },
    Dummy,
}

impl DepChain {
    pub fn get_wait_channel(&mut self) -> Option<crossbeam_channel::Receiver<()>> {
        match self {
            DepChain::Value { v, pred:_ } => {
                let mut v = v.lock().unwrap();
                if v.completed {
                    return None;
                }

                Some(v.get_channel_locked().1)
            },
            DepChain::Dummy => { None }
        }
    }

    pub fn wait(&self) {
        match self {
            DepChain::Value { v, pred:_ } => {
                let mut v = v.lock().unwrap();

                if v.completed {
                    return;
                }

                let rx = v.get_channel_locked().1;
                drop(v);

                rx.recv().unwrap();
            },
            DepChain::Dummy => { }
        }
    }

    pub fn is_completed(&self) -> bool {
        match self {
            DepChain::Value { v, pred:_ } => {
                v.lock().unwrap().completed
            },
            DepChain::Dummy => {
                true
            }
        }
    }

    pub fn new() -> DepChain {
        let v = DepChainV {
            completed: false,
            waiter: None,
        };

        DepChain::Value {
            v: Arc::new(Mutex::new(v)),
            pred: None,
        }
    }

    pub fn new_dummy() -> DepChain {
        DepChain::Dummy
    }

    pub fn complete(&mut self) {
        match self {
            DepChain::Dummy => {},
            DepChain::Value {v, pred:_} => {
                let mut v = v.lock().unwrap();
                let mut v = v.deref_mut();
                v.completed = true;

                let mut b = None;
                std::mem::swap(&mut b, &mut v.waiter);

                if let Some(w) = b {
                    w.0.send(()).unwrap();
                }
            }
        }

    }
}
