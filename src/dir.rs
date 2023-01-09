use crate::error;
use nix::fcntl::OFlag;
use nix::sys::stat::Mode;
use std::ffi::OsStr;
use std::ops::DerefMut;
use std::os::unix::ffi::OsStrExt;
use std::os::unix::io::AsRawFd;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

pub struct DirV {
    parent: Option<Dir>, // None when start directory
    abs_path: PathBuf,
    dirfd: nix::dir::Dir,
}

#[derive(Clone)]
pub struct Dir {
    v: Arc<Mutex<DirV>>,
}

impl Dir {
    pub fn new_at(parent: &Dir, rel_path: &Path) -> Result<Dir, error::E> {
        let parent_dir = parent.v.lock().unwrap();
        let mut abs_path = parent_dir.abs_path.clone();
        abs_path.push(rel_path);

        let fd = error::maybe_open_dir_error(
            &abs_path,
            nix::dir::Dir::openat(
                parent_dir.dirfd.as_raw_fd(),
                rel_path,
                OFlag::O_RDONLY | OFlag::O_DIRECTORY,
                Mode::empty(),
            ),
        )?;

        Ok(Dir {
            v: Arc::new(Mutex::new(DirV {
                dirfd: fd,
                abs_path,
                parent: Some(parent.clone()),
            })),
        })
    }

    pub fn new_root(path: &Path) -> Result<Dir, error::E> {
        let fd = error::maybe_open_dir_error(
            &path,
            nix::dir::Dir::open(path, OFlag::O_RDONLY | OFlag::O_DIRECTORY, Mode::empty()),
        )?;

        Ok(Dir {
            v: Arc::new(Mutex::new(DirV {
                dirfd: fd,
                abs_path: path.to_owned(),
                parent: None,
            })),
        })
    }

    pub fn read_dir_all(&self) -> Result<Vec<nix::dir::Entry>, error::E> {
        let mut v = self.v.lock().unwrap();
        let mut v = v.deref_mut();
        let mut ret = Vec::new();
        let mut pos = 0;
        for e in v.dirfd.iter() {
            let e = error::maybe_readdir_error(&v.abs_path, pos, e)?;
            pos += 1;

            let name = e.file_name();
            let bytes = name.to_bytes();

            if (bytes.len() == 1 && bytes[0] == b'.')
                || (bytes.len() == 2 && bytes[0] == b'.' && bytes[1] == b'.')
            {
                continue;
            }

            ret.push(e);
        }
        Ok(ret)
    }

    pub fn entry_abspath(&self, e: &nix::dir::Entry) -> PathBuf {
        let mut v = self.v.lock().unwrap();
        let mut r = v.abs_path.clone();
        let name = e.file_name();
        let osstr_name = OsStr::from_bytes(name.to_bytes());
        r.push(osstr_name);
        r
    }
}
