use crate::options::Options;
use crate::traverse::Task;
use crossbeam::channel::Sender;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum E {
    SendFreeThreadError {
        e: crossbeam_channel::SendError<Sender<Task>>,
    },
    RecvFreeThreadError {
        e: crossbeam_channel::RecvError,
    },
    OpenDirError {
        path: PathBuf,
        eno: nix::errno::Errno,
    },
    ReadDirError {
        dirpath: PathBuf,
        entry_pos: usize,
        eno: nix::errno::Errno,
    },
    GenericIOError {
        eno: std::io::Error,
    },
}

impl E {
    pub fn is_ignorable_error(&self, opts: &Options) -> bool {
        match self {
            E::OpenDirError { path: _, eno } => match eno {
                nix::errno::Errno::EACCES => {
                    eprintln!("ignored error {:?}", self);
                    opts.ignore_eaccess
                }
                _ => false,
            },
            _ => false,
        }
    }
}

impl From<crossbeam_channel::SendError<Sender<Task>>> for E {
    fn from(e: crossbeam_channel::SendError<Sender<Task>>) -> E {
        E::SendFreeThreadError { e }
    }
}

impl From<crossbeam_channel::RecvError> for E {
    fn from(e: crossbeam_channel::RecvError) -> E {
        E::RecvFreeThreadError { e }
    }
}

pub fn maybe_open_dir_error<V>(abs_path: &Path, r: Result<V, nix::errno::Errno>) -> Result<V, E> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(E::OpenDirError {
            path: abs_path.to_owned(),
            eno: e,
        }),
    }
}

pub fn maybe_readdir_error<V>(
    abs_path: &Path,
    pos: usize,
    r: Result<V, nix::errno::Errno>,
) -> Result<V, E> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(E::ReadDirError {
            dirpath: abs_path.to_owned(),
            entry_pos: pos,
            eno: e,
        }),
    }
}

pub fn maybe_generic_io_error<V>(r: Result<V, std::io::Error>) -> Result<V, E> {
    match r {
        Ok(v) => Ok(v),
        Err(e) => Err(E::GenericIOError { eno: e }),
    }
}
