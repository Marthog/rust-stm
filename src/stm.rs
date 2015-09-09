
use std::cell::RefCell;
use std::sync::{Arc, Semaphore};
use std::sync::atomic::{AtomicBool, Ordering};
use std::mem;
use std::collections::{BTreeMap};
use std::vec::Vec;
use std::collections::btree_map::Entry;
use std::any::Any;

use super::log::{Log};
use super::var::{Var, VarControlBlock};



/// use a thread-local log
///
/// the log is optional and initially None because there is
/// only a log inside of a STM computation
thread_local!(static LOG: RefCell<Option<Log>> = RefCell::new(None));


/// call a STM function from inside of a STM block
/// 
/// if we had control over stack unwinding we could use that
macro_rules! stm_call {
    ( $e:expr )     => ({
        use $crate::stm::StmResult;

        let stm: STM<_> = $e;
        let ret = unsafe { $e.intern_run() };
        match ret {
            Success(s)  => s,
            a@_         => return a,
        }
    })
}


/// declare a block that uses STM
macro_rules! stm {
    ( $e:expr )    => {{
        use $crate::stm::StmResult;

        let func = || {
            let res = $e;
            Success(res)
        };
        STM::new(func)
    }}
}


/// a control block for a currently running STM instance
///
/// STM blocks on all read variables if retry was called
/// this control block is used to let the vars inform the STM instance
///
/// be careful when using this because you can easily create deadlocks
pub struct StmControlBlock {
    /// a semaphore that is used 
    semaphore: Semaphore,

    /// atomic flag indicating that a control block is
    /// dead, meaning that it is no longer needed for waiting
    dead: AtomicBool,
}


impl StmControlBlock {
    /// create a new StmControlBlock
    pub fn new() -> StmControlBlock {
        StmControlBlock {
            semaphore: Semaphore::new(0),
            dead: AtomicBool::new(false),
        }
    }

    /// inform the control block that a variable has changed
    ///
    /// need to be called from outside of STM
    pub fn set_changed(&self) {
        self.semaphore.release();
    }

    /// block until one variable has changed
    ///
    /// may immediately return
    ///
    /// need to be called by the STM
    pub fn wait(&self) {
        self.semaphore.acquire();
        // set as dead
        self.dead.store(true, Ordering::Relaxed);
    }

    /// atomic flag indicating that a control block is
    /// dead, meaning that it is no longer needed for waiting
    pub fn is_dead(&self) -> bool {
        // use relaxed ordering here for more speed
        self.dead.load(Ordering::Relaxed)
    }
}


/// a result of each step of a STM calculation
///
/// it informs of the success or the type of failure
pub enum StmResult<T> {
    /// the call succeeded
    Success(T),

    /// the call failed. immediate return
    ///
    /// is used when a variable is no longer consistent
    Failure,

    /// `retry` was called
    ///
    /// may block until at least one read variable has changed
    Retry,
}


/// call retry in `stm_call!` to let the STM manually run again
///
/// this will block until at least one of the read vars has changed
///
/// # Examples
///
/// ```ignore
/// stm_call!(retry())
/// ```

pub fn retry() -> STM<()> {
    STM::new(|| StmResult::Retry)
}


// /// type synonym for the inner of a STM calculation
type StmFunction<T> = Fn()->StmResult<T>;

/// class representing a STM computation
pub struct STM<T>
{
    /// STM uses a boxed closure internally
    intern: Box<Fn() -> StmResult<T>>
}

impl<T: 'static> STM<T> {

    /// create a new STM calculation from a closure
    pub fn new<F>(func: F) -> STM<T>
        where F: Fn() -> StmResult<T> + 'static
    {
        STM {
            intern: Box::new(func) as Box<StmFunction<T>>,
        }
    }

    /// run a computation and return the result
    ///
    /// internal use only. Prefer atomically because it sets up
    /// the log and retry the computation until it has succeeded
    unsafe fn intern_run(&self) -> StmResult<T> {
        // can't call directly because rust assumes 
        // self.intern() to be a method call
        (&*self.intern)()
    }


    fn log_writeback(&self) -> bool {
        // write data out to variables
        with_log(|log| {
            let mut read_vec = Vec::new();
            let mut write_vec = Vec::new();

            for (k, v) in log.vars.iter() {
                // lock the variable and read the value
                let value = if let Some(ref written) = v.write {
                    // take write lock
                    let lock = k.value.write().unwrap();
                    // get the inner value
                    let value = lock.clone();
                    write_vec.push((written.clone(), lock));
                    value
                } else {
                    let lock = k.value.read().unwrap();
                    let value = lock.clone();
                    read_vec.push(lock);
                    value
                };

                // if the value was read then compare
                if let Some(ref val) = v.read {
                    if !same_address(&value, val) {
                        return false;
                    }
                }
            }

            // now that all are locked commit the changes
            for (value, mut lock) in write_vec {
                *lock = value;
            }

            true
        })
    }


    /// run a STM computation atomically
    pub fn atomically(self) -> T {
        // create a log guard for initializing and cleaning up
        // the log
        let _log_guard = LogGuard::new();

        // loop until success
        loop {
            use self::StmResult::*;

            // run the computation
            let res = unsafe {
                self.intern_run()
            };

            match res {
                // on success exit loop
                Success(t)  => {
                    if self.log_writeback() {
                        return t;
                    }
                }

                // on failure rerun immediately
                Failure     => (),

                // on retry wait for changes
                Retry       => {
                    // create control block for waiting
                    let ctrl = Arc::new(StmControlBlock::new());

                    // access the log to get all used variables
                    with_log(|log| {
                        for (k, v) in log.vars.iter() {
                            if v.read.is_some() {
                                k.wait(ctrl.clone());
                            }
                        }
                        // propably wait until one var has changed
                        ctrl.wait();

                        // let others know that ctrl is dead
                        for (k, v) in log.vars.iter() {
                            if v.read.is_some() {
                                k.set_dead();
                            }
                        }
                    });
                }
            }

            // clear log before retrying computation
            with_log(Log::clear);
        }

        // log_guard removes access
    }



    /// if one of both computations fails with a call to retry
    /// then run the other one
    ///
    /// if both call retry then wait for any of the read cars in
    /// both threads to change
    /*
     * when the first computation fails, immediately rerun in without
     * trying the second one since 'or' provides an alternative to a
     * blocked computation but not for cases when a variable has changed
     * before finishing the computation
     */
    pub fn or(self, other: STM<T>) -> STM<T> {
        let func = move || {
            use self::StmResult::*;

            // create a backup of the log
            let backup = with_log(|log| log.clone());

            // run the first computation
            let s = unsafe { self.intern_run() };
            
            match s {
                // return success and failure
                a@Success(_)    => a,
                Failure         => Failure,
                // run other on retry
                Retry           => {
                    // use backup of log
                    let old_log = with_log(|log| mem::replace(log, backup));
                    // run other
                    let o = unsafe { other.intern_run() };

                    // if both called retry then exit
                    if let Retry = o {
                        // combine both logs so that all reads are considered
                        with_log(|log| log.combine_after_retry(old_log));
                    }
                    o
                }
            }
        };

        STM::new(func)
    }
}


/// apply a function f to the log and return the result
///
/// will panic when called from outside of a STM computation
pub fn with_log<F, R>(f: F) -> R
    where F: FnOnce(&mut Log) -> R
{
    LOG.with(|cell| {
        let mut inner = cell.borrow_mut();
        let mut inner = inner.as_mut().expect("with_log called out of STM");
        f(&mut inner)
    })
}


fn arc_to_address<T: ?Sized>(arc: &Arc<T>) -> usize {
    &**arc as *const T as *const u32 as usize
}

fn same_address<T: ?Sized>(a: &Arc<T>, b: &Arc<T>) -> bool {
    arc_to_address(a)==arc_to_address(b)
}

/// RAII guard for enclosing a atomic operation
/// 
/// `new` initialized a log and drop destroys if.
///
/// # Panics
///
/// when a log guard is created when another one exists
///
/// don't use nested STM computations
#[must_use]
struct LogGuard;

impl LogGuard {
    pub fn new() -> LogGuard {
        // init log
        LOG.with(|cell| {
            let mut inner = cell.borrow_mut();

            // ensure that there is just one STM at a time
            assert!(inner.is_none(), "STM: already in atomic operation");

            // set log
            *inner = Some(Log::new());
        });
        LogGuard
    }
}

impl Drop for LogGuard {
    fn drop(&mut self) {
        // delete log after usage
        LOG.with(|cell| {
            let mut inner = cell.borrow_mut();
            // ensure that the inner is present
            debug_assert!(inner.is_some());

            // remove log
            *inner = None;
        });
    }
}



#[test]
#[should_panic]
// call with_log when it is not initialized
fn test_with_log_no_stm() {
    with_log(|_| ());
}

#[test]
// test if creation and destruction of the LogGuard works
fn test_log_guard() {
    let _ = LogGuard::new();
}

#[test]
fn test_read_var() {
    let guard = LogGuard::new();
    let var = Var::new(vec![1,2]);
    let x = var.read();
    
    assert_eq!(x, [1,2]);
}

#[test]
fn test_stm_simple() {
    let stm = STM::new(|| StmResult::Success(42));
    let x = stm.atomically();
    assert_eq!(x, 42);
}


#[test]
fn test_stm_read() {
    let read = Var::new(42);

    let stm = STM::new(move || {
        let r = read.read();
        StmResult::Success(r)
    });
    let x = stm.atomically();

    assert_eq!(x, 42);
}

#[test]
fn test_stm_write() {
    let write = Var::new(42);

    let writecp = write.clone();
    let stm = STM::new(move || {
        writecp.write(0);
        StmResult::Success(())
    });
    let _ = stm.atomically();

    assert_eq!(write.read_immediate(), 0);
}

#[test]
fn test_stm_copy() {
    let read = Var::new(42);
    let write = Var::new(0);

    let writecp = write.clone();
    let stm = STM::new(move || {
        let r = read.read();
        writecp.write(r);
        StmResult::Success(())
    });
    stm.atomically();

    assert_eq!(write.read_immediate(), 42);
}
