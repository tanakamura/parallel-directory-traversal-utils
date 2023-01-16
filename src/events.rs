use std::ops::{Deref,DerefMut};
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
        if self.waiter.is_none() {
            let ret = crossbeam_channel::bounded(0);
            self.waiter = Some(ret);
        }

        return self.waiter.clone().unwrap();
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

pub type WaitChan = crossbeam_channel::Receiver<()>;

pub struct CompleteTestResult {
    pub completed: bool,
    pub wait_chan: Option<WaitChan>
}

impl DepChain {
    pub fn is_completed(&self, get_channel: bool) -> CompleteTestResult {
        match self {
            DepChain::Value { v, pred:_ } => {
                let mut v = v.lock().unwrap();
                if v.completed {
                    CompleteTestResult { completed:true, wait_chan: None}
                } else {
                    if get_channel {
                        let chan = v.get_channel_locked().1;
                        CompleteTestResult { completed:false, wait_chan: Some(chan)}
                    } else {
                        CompleteTestResult { completed:false, wait_chan: None }
                    }
                }
            },
            DepChain::Dummy => {
                CompleteTestResult { completed:true, wait_chan: None }
            }
        }
    }

    pub fn wait(&self) {
        let cr = self.is_completed(true);

        if cr.completed {
            return;
        }

        cr.wait_chan.unwrap().recv().unwrap();
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

                println!("notify {:?}", v as *const DepChainV);

                let mut b = None;
                std::mem::swap(&mut b, &mut v.waiter);

                if let Some(w) = b {
                    w.0.send(()).unwrap();
                }
            }
        }
    }

    pub fn get_ptr(&self) -> *const DepChainV {
        match self {
            DepChain::Dummy => std::ptr::null(),
            DepChain::Value {v, pred:_} => {
                let v = v.lock().unwrap();
                let v = v.deref();
                v as *const DepChainV
            }
        }
    }
}
