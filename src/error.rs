use crossbeam::channel::Sender;
use crate::traverse::Task;

#[derive(Debug)]
pub enum E {
    SendFreeThreadError { e: crossbeam_channel::SendError<Sender<Task>> },
    RecvFreeThreadError { e: crossbeam_channel::RecvError }
}

impl From<crossbeam_channel::SendError<Sender<Task>>> for E {
    fn from ( e :crossbeam_channel::SendError<Sender<Task>>) -> E {
        E::SendFreeThreadError {
            e
        }
    }
}

impl From<crossbeam_channel::RecvError> for E {
    fn from ( e :crossbeam_channel::RecvError) -> E {
        E::RecvFreeThreadError {
            e
        }
    }
}
