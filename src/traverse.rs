use crate::dir::Dir;
use crate::error;
use crate::events;
use crate::options::{Options, Order};
use crossbeam::channel::{select, Receiver, Sender};
use events::CompleteTestResult;
use std::cell::RefCell;
use std::ffi::OsString;
use std::io::Write;
use std::os::unix::ffi::OsStringExt;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::thread;

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

#[derive(Clone, Debug)]
pub struct ReorderKey(Vec<usize>);

struct DepPostProcs {
    current: bool,
    pred: events::DepChain,
    succ: events::DepChain,
    postprocs: Vec<TaskPostProc>,
    key: ReorderKey,
}

impl PartialOrd for DepPostProcs {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.key.0.cmp(&other.key.0))
    }
}

impl Ord for DepPostProcs {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        let lenl = self.key.0.len();
        let lenr = other.key.0.len();

        let len = std::cmp::min(lenl, lenr);
        for i in 0..len {
            match self.key.0[i].cmp(&other.key.0[i]) {
                std::cmp::Ordering::Equal => (),
                non_eq => return non_eq,
            }
        }

        lenr.cmp(&lenl) // longer vector is lesser
    }
}

impl PartialEq for DepPostProcs {
    fn eq(&self, other: &Self) -> bool {
        self.key.0.eq(&other.key.0)
    }
}
impl Eq for DepPostProcs {}

impl ReorderKey {
    fn inc(&mut self) {
        let len = self.0.len();
        self.0[len - 1] += 1;
    }

    fn push(&mut self) {
        self.0.push(0);
    }
}

struct TraverseState<'a> {
    opts: &'a Options,
    pendings: std::collections::BTreeSet<Rc<RefCell<DepPostProcs>>>,
    current: Rc<RefCell<DepPostProcs>>,
    tid: usize,
    current_key: ReorderKey,
}

impl DepPostProcs {
    fn flush_postprocs(&mut self) -> Result<(), error::E> {
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
        let mut cur = self.current.borrow_mut();

        let mut v = Vec::new();
        std::mem::swap(&mut v, &mut cur.postprocs);
        for t in v {
            run_postproc_task(t)?;
        }

        Ok(())
    }

    fn push_postproc(&mut self, t: TaskPostProc) -> Result<(), error::E> {
        self.pump(false)?;
        let mut cur = self.current.borrow_mut();
        if cur.pred.is_completed(false).completed {
            cur.flush_postprocs()?;
            run_postproc_task(t)?;
        } else {
            cur.postprocs.push(t);
        }
        Ok(())
    }

    fn pump(&mut self, get_wait_channel: bool) -> Result<CompleteTestResult, error::E> {
        loop {
            if let Some(v) = self.pendings.first() {
                let mut v = v.borrow_mut();
                //println!("{}: pump top={:?}, key={:?}", self.tid, v.pred.get_ptr(), v.key);
                let r = v.pred.is_completed(get_wait_channel);
                if r.completed {
                    if v.current {
                        v.flush_postprocs()?;
                        return Ok(CompleteTestResult {
                            completed: true,
                            wait_chan: None,
                        });
                    } else {
                        drop(v);
                        let v = self.pendings.pop_first().unwrap();
                        let mut v = v.borrow_mut();

                        v.flush_postprocs()?;
                        v.succ.notify_complete()
                    }
                } else {
                    return Ok(r);
                }
            } else {
                self.flush_cur_postprocs()?;
                return Ok(CompleteTestResult {
                    completed: true,
                    wait_chan: None,
                });
            }
        }
    }

    fn gen_chain(&mut self) -> (events::DepChain, events::DepChain, ReorderKey) {
        // (pred,succ)

        let new_task_pred = crate::events::DepChain::new();
        let new_task_succ = crate::events::DepChain::new();

        let push_val = Rc::new(RefCell::new(DepPostProcs {
            current: true,
            pred: new_task_succ.clone(),
            succ: events::DepChain::new_dummy(),
            postprocs: Vec::new(),
            key: self.current_key.clone(),
        }));

        let mut cur = self.current.borrow_mut();
        cur.fixup(new_task_pred.clone());
        drop(cur);

        self.current = push_val.clone();
        self.pendings.insert(push_val);

        let mut newkey = self.current_key.clone();
        newkey.push();
        self.current_key.inc();

        (new_task_pred, new_task_succ, newkey)
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

    //println!("{}: traverse dir {:?}", st.tid, path);

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
                                st.pump(false)?;
                                let (new_pred, new_succ, new_key) = st.gen_chain();

                                let read_child = Task::ReadDir {
                                    parent_dir: Some(d.clone()),
                                    path: crate::pathstr::entry_to_path(&e).to_owned(),
                                    dep_pred: new_pred,
                                    dep_succ: new_succ,
                                    key: new_key,
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
            mut key,
        } => {
            //println!("{}: start path={:?}, pred={:?}, succ={:?}, key={:?}",
            //         st.tid,
            //         path,
            //         dep_pred.get_ptr(),
            //         dep_succ.get_ptr(),
            //         key);

            let cur_dep = Rc::new(RefCell::new(DepPostProcs {
                current: true,
                pred: dep_pred,
                succ: crate::events::DepChain::new_dummy(),
                postprocs: Vec::new(),
                key: key.clone(),
            }));

            key.inc();
            st.current_key = key;
            st.pendings.insert(cur_dep.clone());
            st.current = cur_dep;

            //println!("{}:traverse start {:?} {:?}", st.tid, path, st.current_key);
            let mut err = traverse_dir(st, &free_thread_queue_rx, parent_dir.as_ref(), &path);
            //println!("{}:traverse finish {:?}", st.tid, path);

            if let Ok(_) = err {
                let mut cur = st.current.borrow_mut();
                cur.fixup(dep_succ);
                drop(cur);
                err = st.pump(false).map(|_| ());
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
                pendings: std::collections::BTreeSet::new(),
                current: Rc::new(RefCell::new(DepPostProcs {
                    current: true,
                    pred: crate::events::DepChain::new_dummy(),
                    succ: crate::events::DepChain::new_dummy(),
                    postprocs: Vec::new(),
                    key: ReorderKey(vec![]),
                })), // dummy, unused value
                current_key: ReorderKey(vec![0]),
            };

            let tq = crossbeam::channel::bounded(1);
            let mut ret: Result<(), error::E> = Ok(());

            loop {
                free_thread_queue.0.send(tq.0.clone())?;

                let tv;

                loop {
                    //println!("{}: XX", st.tid);
                    let pr = st.pump(true)?;

                    if pr.completed {
                        //println!("{}: pump cmplete", st.tid);

                        tv = tq.1.recv()?;
                        break;
                    } else {
                        //println!("{}: pump pending", st.tid);

                        let chan = pr.wait_chan.unwrap();
                        select! {
                            recv(chan) -> v => {v?; continue;},
                            recv(tq.1) -> v => {tv = v?;  break;}
                        }
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
        key: ReorderKey,
    },
    #[cfg(test)]
    Nop,

    Quit,
}

pub fn traverse(t: &mut Traverser) -> Result<(), error::E> {
    let tl = ThreadList::new(t.opt.clone());

    let final_dep = events::DepChain::new();
    let mut root_first = events::DepChain::new();

    root_first.notify_complete();

    let read_root = Task::ReadDir {
        parent_dir: None,
        path: t.opt.src_path.to_owned(),
        dep_pred: root_first,
        dep_succ: final_dep.clone(),
        key: ReorderKey(vec![0]),
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
