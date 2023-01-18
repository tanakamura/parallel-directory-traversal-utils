use std::pin::Pin;
use std::future::Future;

use crate::error;

pub type TaskOutput = Result<(),error::E>;

pub struct Task {
    f: Pin<Box<dyn Future<Output=TaskOutput>>>
}

impl Task {
    pub fn new(f : Pin<Box<dyn Future<Output=TaskOutput>>>) -> Task {
        Task { f }
    }
}


