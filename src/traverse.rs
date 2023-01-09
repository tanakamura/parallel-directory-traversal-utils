use crate::dir::Dir;
use crate::error;
use crate::events;
use crate::events::DepChain;
use crate::options::{Options, Order};
use crossbeam::channel::{Receiver, Sender};
use std::ffi::OsString;
use std::os::unix::ffi::OsStringExt;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::thread;
use std::ops::DerefMut;

pub struct TraverseThread {
    thread: std::thread::JoinHandle<Result<(), error::E>>,
}

pub enum TaskPostProc {
    Show(OsString),
    DepChain(DepChain),
    Dummy
}

struct TraverseContext {
    d: DepChain,
    cnt: usize,
}

fn run_postproc_task(mut t: TaskPostProc) -> Result<(),error::E> {
    let mut stk = Vec::new();   // maintain traverse context, recursive implementation causes stack overflow

    loop {
        match t {
            TaskPostProc::Show(s) => {
                let mut v = s.into_vec();
                v.push(b'\n');
                error::maybe_generic_io_error(std::io::stdout().write_all(v.as_slice()))?;
            },
            TaskPostProc::DepChain(d) => {
                stk.push(TraverseContext{d, cnt: 0});
            },
            TaskPostProc::Dummy => {
                break;
            }
        }

        t = TaskPostProc::Dummy;
        while stk.len() != 0 {
            let len = stk.len();
            let mut top = &mut stk[len-1];
            let mut top_d = top.d.v.lock().unwrap();
            let cbs = &top_d.complete_callbacks;
            let l = cbs.len();
            let cnt = top.cnt;

            if l == cnt {
                /* finish */
                top_d.complete = true;
                if let Some(b) = &top_d.waiter {
                    // all finished
                    b.wait();
                }
                drop(top_d);
                drop(top);

                stk.pop();
                continue;
            } else {
                let mut tnext = TaskPostProc::Dummy;
                std::mem::swap(&mut top_d.complete_callbacks[cnt], &mut tnext);

                top.cnt = cnt + 1;

                t = tnext;
            }
        }
    }

    Ok(())
}

fn push_postproc(dep_pred: &Option<DepChain>, postprocs: &mut Vec<TaskPostProc>, t: TaskPostProc) -> Result<(),error::E> {
    if dep_pred.is_some() {
        postprocs.push(t);
    } else {
        run_postproc_task(t)?;
    }
    Ok(())
}

fn traverse_dir(
    opts: &Options,
    free_thread_queue_rx: &Receiver<Sender<Task>>,
    parent_dirfd: Option<&Dir>,
    path: &Path,
    mut dep_pred: Option<DepChain>,
    mut postprocs: &mut Vec<TaskPostProc>,
) -> Result<Option<DepChain>, error::E> {
    let d = if let Some(pd) = parent_dirfd {
        Dir::new_at(&pd, &path)
    } else {
        Dir::new_root(&path)
    };

    match d {
        Err(e) => {
            if e.is_ignorable_error(&opts) {
                return Ok(dep_pred);
            } else {
                return Err(e);
            }
        }

        Ok(d) => {
            let mut entries = d.read_dir_all()?;

            if opts.order == Order::Alphabetical {
                entries.sort_by(|l, r| l.file_name().cmp(r.file_name()));
            }

            for e in entries {
                let t = e.file_type().unwrap();
                match t {
                    nix::dir::Type::File => {
                        if opts.method == crate::options::Method::List {
                            let abspath = d.entry_abspath(&e);
                            let path_str = abspath.into_os_string();
                            push_postproc(&dep_pred, &mut postprocs, TaskPostProc::Show(path_str))?;
                        }
                    }
                    nix::dir::Type::Directory => {
                        if opts.method == crate::options::Method::List {
                            let abspath = d.entry_abspath(&e);
                            let path_str = abspath.clone().into_os_string();
                            push_postproc(&dep_pred, &mut postprocs, TaskPostProc::Show(path_str))?;
                        }

                        let nt = free_thread_queue_rx.try_recv();

                        match nt {
                            Ok(t) => {
                                let mut postprocs2 = Vec::new();
                                std::mem::swap(&mut postprocs2, &mut postprocs);

                                if let Some(d) = & dep_pred {
                                    d.add_complete_postproc(postprocs2);
                                } else {
                                    postproc(postprocs2)?;
                                }

                                let next_dep = events::DepChain::new();

                                let read_child = Task::ReadDir {
                                    parent_dir: Some(d.clone()),
                                    path: crate::pathstr::entry_to_path(&e).to_owned(),
                                    dep_pred,
                                    dep_succ: next_dep.clone(),
                                };

                                t.send(read_child).unwrap();

                                dep_pred = Some(next_dep);
                            },

                            Err(_) => { // traverse in own thread
                                dep_pred = traverse_dir(
                                    opts,
                                    &free_thread_queue_rx,
                                    Some(&d),
                                    crate::pathstr::entry_to_path(&e),
                                    dep_pred,
                                    &mut postprocs,
                                )?;
                            },
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(dep_pred)
}

pub fn postproc(p: Vec<TaskPostProc>) -> Result<(),error::E> {
    for v in p {
        run_postproc_task(v)?;
    }
    Ok(())
}

fn run_1task(
    t: Task,
    opts: &Options,
    free_thread_queue_rx: &Receiver<Sender<Task>>,
) -> Result<bool, error::E> {
    match t {
#[cfg(test)]
        Task::Nop => {}
        Task::Quit => return Ok(true),
        Task::ReadDir {
            parent_dir,
            path,
            dep_pred,
            dep_succ,
        } => {
            let mut postprocs = Vec::new();

            let dep_pred = traverse_dir(
                &opts,
                &free_thread_queue_rx,
                parent_dir.as_ref(),
                &path,
                dep_pred,
                &mut postprocs,
            );

            postprocs.push(TaskPostProc::DepChain(dep_succ));

            match dep_pred {
                Ok(Some(p)) => {
                    p.add_complete_postproc(postprocs);
                }
                Ok(None) => {
                    postproc(postprocs);
                }
                Err(e) => {
                    postproc(postprocs);
                    return Err(e);
                }
            }
        }
    };

    Ok(false)
}

impl TraverseThread {
    fn new(
        opts: Options,
        free_thread_queue: (Sender<Sender<Task>>, Receiver<Sender<Task>>),
    ) -> TraverseThread {
        let th = thread::spawn(move || -> Result<(), error::E> {
            let tq = crossbeam::channel::unbounded();
            let mut ret: Result<(), error::E> = Ok(());

            loop {
                free_thread_queue.0.send(tq.0.clone())?;
                let t = tq.1.recv()?;

                match t {
                    Task::Quit => {
                        break;
                    }
                    _ => {
                        if ret.is_ok() {
                            let r = run_1task(t, &opts, &free_thread_queue.1);
                            match r {
                                Ok(true) => break,
                                Ok(false) => continue,
                                Err(e) => ret = Err(e),
                            }
                        }
                    }
                }
            }

            ret
        });

        TraverseThread { thread: th }
    }
}

pub struct ThreadList {
    free_thread_queue: (Sender<Sender<Task>>, Receiver<Sender<Task>>),
    threads: Vec<TraverseThread>,
}

impl ThreadList {
    fn new(opts: Options) -> ThreadList {
        let mut v = Vec::new();
        let free_thread_queue = crossbeam::channel::unbounded();

        for _ in 0..opts.num_threads {
            v.push(TraverseThread::new(opts.clone(), free_thread_queue.clone()));
        }

        ThreadList {
            free_thread_queue,
            threads: v,
        }
    }

    fn pop_free_thread(&self) -> Result<Sender<Task>, error::E> {
        let e = self.free_thread_queue.1.recv()?;
        Ok(e)
    }
}

impl Drop for ThreadList {
    fn drop(&mut self) {
        let n = self.threads.len();
        for _ in 0..n {
            let t = self.pop_free_thread().unwrap();
            t.send(Task::Quit).unwrap();
        }

        for _ in 0..n {
            let t = self.threads.pop().unwrap();
            let r = t.thread.join().unwrap();
            if r.is_err() {
                println!("{:?}", r);
            }
        }
    }
}

pub struct Traverser {
    pub opt: Options,
}

type TaskStack = Vec<Task>;

pub enum Task {
    ReadDir {
        parent_dir: Option<crate::dir::Dir>, // None if root
        path: PathBuf,
        dep_pred: Option<events::DepChain>,
        dep_succ: events::DepChain,
    },
#[cfg(test)]
    Nop,

    Quit,
}

pub fn traverse(t: &mut Traverser) -> Result<(), error::E> {
    let tl = ThreadList::new(t.opt.clone());

    let final_dep = events::DepChain::new();

    let read_root = Task::ReadDir {
        parent_dir: None,
        path: t.opt.src_path.to_owned(),
        dep_pred: None,
        dep_succ: final_dep.clone(),
    };

    let ft = tl.pop_free_thread()?;

    ft.send(read_root).unwrap();

    final_dep.wait();

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn t() -> Result<(), error::E> {
        let mut opts = crate::options::test_option(".");
        let nthread = 16;
        opts.num_threads = 16;
        let tl = ThreadList::new(opts);

        for i in 0..4096 {
            let f = tl.pop_free_thread()?;
            f.send(Task::Nop);
        }

        Ok(())
    }
}
