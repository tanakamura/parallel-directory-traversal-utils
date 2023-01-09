use std::sync::{Arc, Mutex};
use std::ops::DerefMut;

pub struct DepChainV {
    complete: bool,
    waiter: Option<Arc<std::sync::Barrier>>, // to notify all complete
    complete_callbacks: Vec<crate::traverse::TaskPostProc>,
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

    pub fn notify_complete(&self)->Result<(),crate::error::E> {
        let mut v = self.v.lock().unwrap();
        let mut v = v.deref_mut();
        v.complete = true;
        if let Some(b) = &v.waiter {
            // all finished
            b.wait();
        }

        let mut v2 = Vec::new();
        std::mem::swap(&mut v2, &mut v.complete_callbacks);

        crate::traverse::postproc(v2)?;

        Ok(())
    }

    pub fn add_complete_postproc(&self, t: Vec<crate::traverse::TaskPostProc>)
    {
        let mut v = self.v.lock().unwrap();
        if v.complete {
            crate::traverse::postproc(t);
        } else {
            for t in t.into_iter() {
                v.complete_callbacks.push(t);
            }
        }
    }
}
