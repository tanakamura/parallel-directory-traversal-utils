use crate::dir::Dir;
use crate::error;
use crate::events;
use crate::options::{Options, Order};
use crossbeam::channel::{select, Receiver, Sender};
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::thread;
use std::rc::Rc;
use std::cell::RefCell;

pub struct TraverseThread {
    thread: std::thread::JoinHandle<Result<(), error::E>>,
}

#[derive(Debug)]
pub enum TaskPostProc {
    Show(OsString),
}

fn run_postproc_task(t: TaskPostProc) -> Result<(), error::E> {
    match t {
        TaskPostProc::Show(s) => {
            let mut v = s.into_vec();
            v.push(b'\n');
            error::maybe_generic_io_error(std::io::stdout().write_all(v.as_slice()))?;
        }
    }

    Ok(())
}

struct DepPostProcs {
    current: bool,
    pred: events::DepChain,
    succ: events::DepChain,
    postprocs: Vec<TaskPostProc>,
}

struct TraverseState<'a> {
    opts: &'a Options,
    pend_fifo: std::collections::VecDeque<Rc<RefCell<DepPostProcs>>>,
    current: Rc<RefCell<DepPostProcs>>,
    tid: usize
}

impl DepPostProcs {
    fn flush_postprocs(&mut self) -> Result<(),error::E> {
        let tmp_vec = std::mem::take(&mut self.postprocs);
        for t in tmp_vec {
            run_postproc_task(t)?;
        }
        Ok(())
    }

    fn fixup(&mut self, succ: events::DepChain) {
        self.succ = succ;
        self.current = false;
    }
}

impl<'a> TraverseState<'a> {
    fn flush_cur_postprocs(&mut self) -> Result<(), error::E> {
        assert!(self.pend_fifo.len() == 0);

        let mut cur = self.current.borrow_mut();

        let mut v = Vec::new();
        std::mem::swap(&mut v, &mut cur.postprocs);
        for t in v {
            run_postproc_task(t)?;
        }

        Ok(())
    }

    fn push_postproc(&mut self, t: TaskPostProc) -> Result<(), error::E> {
        self.pump()?;
        let mut cur = self.current.borrow_mut();
        if cur.pred.is_completed() {
            cur.flush_postprocs()?;
            run_postproc_task(t)?;
        } else {
            cur.postprocs.push(t);
        }
        Ok(())
    }

    fn pump(&mut self) -> Result<bool, error::E> {
        loop {
            if let Some(v) = self.pend_fifo.get(0) {
                let mut v = v.borrow_mut();
                println!("{}: pump top={:?}", self.tid, v.pred.get_ptr());
                if v.pred.is_completed() {
                    if v.current {
                        v.flush_postprocs()?;
                        return Ok(false);
                    } else {
                        drop(v);
                        let v = self.pend_fifo.pop_front().unwrap();
                        let mut v = v.borrow_mut();

                        v.flush_postprocs()?;
                        v.succ.complete()
                    }
                } else {
                    return Ok(false);
                }
            } else {
                self.flush_cur_postprocs()?;
                return Ok(true);
            }
        }
    }

    fn gen_chain(&mut self) -> (crate::events::DepChain, crate::events::DepChain) {
        // (pred,succ)

        let new_task_pred = crate::events::DepChain::new();
        let new_task_succ = crate::events::DepChain::new();

        let push_val = Rc::new(RefCell::new(DepPostProcs {
            current: true,
            pred: new_task_succ.clone(),
            succ: events::DepChain::new_dummy(),
            postprocs: Vec::new()
        }));

        let mut cur = self.current.borrow_mut();
        cur.fixup(new_task_pred.clone());
        drop(cur);

        self.current = push_val.clone();
        self.pend_fifo.push_back(push_val);

        (new_task_pred, new_task_succ)
    }
}

fn traverse_dir(
    st: &mut TraverseState,
    free_thread_queue_rx: &Receiver<Sender<Task>>,
    parent_dirfd: Option<&Dir>,
    path: &Path,
) -> Result<(), crate::error::E> {
    let d = if let Some(pd) = parent_dirfd {
        Dir::new_at(&pd, &path)
    } else {
        Dir::new_root(&path)
    };

    println!("{}: traverse dir {:?}", st.tid, path);

    match d {
        Err(e) => {
            if e.is_ignorable_error(&st.opts) {
                return Ok(());
            } else {
                return Err(e);
            }
        }

        Ok(d) => {
            let mut entries = d.read_dir_all()?;

            if st.opts.order == Order::Alphabetical {
                entries.sort_by(|l, r| l.file_name().cmp(r.file_name()));
            }

            for e in entries {
                let t = e.file_type().unwrap();

                if st.opts.method == crate::options::Method::List {
                    let abspath = d.entry_abspath(&e);
                    let path_str = abspath.into_os_string();
                    st.push_postproc(TaskPostProc::Show(path_str))?;
                }

                match t {
                    nix::dir::Type::Directory => {
                        let nt = free_thread_queue_rx.try_recv();

                        match nt {
                            Ok(t) => {
                                st.pump()?;
                                let (new_pred, new_succ) = st.gen_chain();

                                let read_child = Task::ReadDir {
                                    parent_dir: Some(d.clone()),
                                    path: crate::pathstr::entry_to_path(&e).to_owned(),
                                    dep_pred: new_pred,
                                    dep_succ: new_succ,
                                };

                                t.send(read_child).unwrap();
                            }

                            Err(_) => {
                                // traverse in own thread
                                traverse_dir(
                                    st,
                                    &free_thread_queue_rx,
                                    Some(&d),
                                    crate::pathstr::entry_to_path(&e),
                                )?;
                            }
                        }
                    }
                    _ => {}
                }
            }
        }
    }

    Ok(())
}

//pub fn postproc(p: Vec<TaskPostProc>) -> Result<(), error::E> {
//    for v in p {
//        run_postproc_task(v)?;
//    }
//    Ok(())
//}

fn run_1task(
    t: Task,
    st: &mut TraverseState,
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
            let cur_dep = Rc::new(RefCell::new(DepPostProcs {
                current: true,
                pred: dep_pred,
                succ: crate::events::DepChain::new_dummy(),
                postprocs: Vec::new()
            }));

            st.pend_fifo.push_back(cur_dep.clone());
            st.current = cur_dep;

            println!("{}:traverse start {:?}", st.tid, path);
            let mut err = traverse_dir(st, &free_thread_queue_rx, parent_dir.as_ref(), &path);
            println!("{}:traverse finish {:?}", st.tid, path);

            if let Ok(_) = err {
                let mut cur = st.current.borrow_mut();
                cur.fixup(dep_succ);
                drop(cur);
                err = st.pump().map(|_| ());
            }

            if let Err(e) = err {
                return Err(e);
            }
        }
    };

    Ok(false)
}

impl TraverseThread {
    fn new(
        opts: Options,
        free_thread_queue: (Sender<Sender<Task>>, Receiver<Sender<Task>>),
        tid: usize,
    ) -> TraverseThread {
        let th = thread::spawn(move || -> Result<(), error::E> {
            let mut st = TraverseState {
                tid,
                opts: &opts,
                pend_fifo: std::collections::VecDeque::new(),
                current : Rc::new(RefCell::new(DepPostProcs {
                    current: true,
                    pred: crate::events::DepChain::new_dummy(),
                    succ: crate::events::DepChain::new_dummy(),
                    postprocs: Vec::new()
                }))             // dummy, unused value
            };

            let tq = crossbeam::channel::bounded(0);
            let mut ret: Result<(), error::E> = Ok(());

            loop {
                free_thread_queue.0.send(tq.0.clone())?;

                let tv;

                loop {
                    st.pump()?;

                    if let Some(top) = st.pend_fifo.get_mut(0) {
                        let mut top_mut = top.borrow_mut();
                        let chan = top_mut.pred.get_wait_channel();
                        if let Some(chan) = chan {
                            select! {
                                recv(chan) -> v => {v?; continue;},
                                recv(tq.1) -> v => {tv = v?;  break;}
                            }
                        } else {
                            continue;
                        }
                    } else {
                        tv = tq.1.recv()?;
                        break;
                    }
                }

                match tv {
                    Task::Quit => {
                        break;
                    }
                    _ => {
                        if ret.is_ok() {
                            let r = run_1task(tv, &mut st, &free_thread_queue.1);
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

        for id in 0..opts.num_threads {
            v.push(TraverseThread::new(
                opts.clone(),
                free_thread_queue.clone(),
                id,
            ));
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

pub enum Task {
    ReadDir {
        parent_dir: Option<crate::dir::Dir>, // None if root
        path: PathBuf,
        dep_pred: events::DepChain,
        dep_succ: events::DepChain,
    },
    #[cfg(test)]
    Nop,

    Quit,
}

pub fn traverse(t: &mut Traverser) -> Result<(), error::E> {
    let tl = ThreadList::new(t.opt.clone());

    let final_dep = events::DepChain::new();
    let mut root_first = events::DepChain::new();

    root_first.complete();

    let read_root = Task::ReadDir {
        parent_dir: None,
        path: t.opt.src_path.to_owned(),
        dep_pred: root_first,
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
