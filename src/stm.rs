
use std::cell::RefCell;
use std::sync::{Arc, Mutex, Condvar};
use std::sync::atomic::{AtomicBool, Ordering};
use std::mem;
use std::vec::Vec;

use super::log::{Log};

#[cfg(test)]
use super::var::{Var};



/// use a thread-local log
///
/// the log is optional and initially None because there is
/// only a log inside of a STM computation
thread_local!(static LOG: RefCell<Option<Log>> = RefCell::new(None));


/// a control block for a currently running STM instance
///
/// STM blocks on all read variables if retry was called
/// this control block is used to let the vars inform the STM instance
///
/// be careful when using this because you can easily create deadlocks
pub struct StmControlBlock {
    // a simple binary semaphore to unblock

    /// boolean storing true if a still blocked
    /// it can be put in the mutex but that may
    /// block a thread that is currently releasing
    /// multiple variables on writing that value
    blocked: AtomicBool,

    /// a lock needed for the condition variable
    lock: Mutex<()>,

    /// condition variable that is used for pausing and
    /// waking the thread
    wait_cvar: Condvar,

    /// atomic flag indicating that a control block is
    /// dead, meaning that it is no longer needed for waiting
    dead: AtomicBool,
}


impl StmControlBlock {
    /// create a new StmControlBlock
    pub fn new() -> StmControlBlock {
        StmControlBlock {
            blocked: AtomicBool::new(true),
            lock: Mutex::new(()),
            wait_cvar: Condvar::new(),
            dead: AtomicBool::new(false),
        }
    }

    /// inform the control block that a variable has changed
    ///
    /// need to be called from outside of STM
    pub fn set_changed(&self) {
        // unblock
        self.blocked.store(false, Ordering::SeqCst);
        // wake thread
        self.wait_cvar.notify_one();
    }

    /// block until one variable has changed
    ///
    /// may immediately return
    ///
    /// need to be called by the STM
    pub fn wait(&self) {
        let mut blocked = self.blocked.load(Ordering::SeqCst);
        let mut lock = self.lock.lock().unwrap();
        while blocked {
            lock = self.wait_cvar.wait(lock).unwrap();
            blocked = self.blocked.load(Ordering::SeqCst);
        }
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
/// ```
/// # #[macro_use] extern crate stm;
/// # fn main() {
/// use stm::retry;
/// let infinite_retry = stm!({
///     stm_call!(retry());
/// });
/// # }
/// ```

pub fn retry() -> STM<'static, ()> {
    STM::new(|| StmResult::Retry)
}

/// type synonym for the inner of a STM calculation
type StmFunction<'a, T> = Fn() -> StmResult<T> + 'a;

/// class representing a STM computation
pub struct STM<'a, T>
{
    /// STM uses a boxed closure internally
    intern: Box<Fn() -> StmResult<T> + 'a>
}

impl<'a, T: 'a> STM<'a, T> {

    /// create a new STM calculation from a closure
    pub fn new<F>(func: F) -> STM<'a, T>
        where F: Fn() -> StmResult<T> + 'a
    {
        STM {
            intern: Box::new(func) as Box<StmFunction<'a, T>>,
        }
    }

    /// run a computation and return the result
    ///
    /// internal use only. Prefer atomically because it sets up
    /// the log and retry the computation until it has succeeded
    ///
    /// internal use only
    pub fn intern_run(&self) -> StmResult<T> {
        // can't call directly because rust assumes 
        // self.intern() to be a method call
        (self.intern)()
    }


    /// write the log back to the variables
    ///
    /// return true for success and false if a read var has changed
    fn log_writeback(&self) -> bool {
        // use two phase locking for safely writing data back to the vars
        with_log(|log| {

            // first phase: acquire locks
            // check for correctness of the values and perform
            // an early return if something is not consistent
            

            // created arrays for storing the locks
            
            // vector of locks
            let mut read_vec = Vec::new();

            // vector of tuple (variable, value, lock)
            let mut write_vec = Vec::new();

            for (var, value) in &log.vars {
                // lock the variable and read the value
                let current_value =
                    match value.write {
                    Some(ref written) => {
                        // take write lock
                        let lock = var.value.write().unwrap();
                        // get the current value
                        let current_value = lock.clone();
                        // add all data to the vector
                        write_vec.push((var, written.clone(), lock));
                        // return the current value
                        current_value
                    }
                    _ => {
                        // take a read lock
                        let lock = var.value.read().unwrap();
                        // take the current value
                        let current_value = lock.clone();
                        read_vec.push(lock);
                        current_value
                    }
                };

                // if the value was read then compare
                if let Some(ref original) = value.read {
                    // if the current value is no longer that
                    // when the computation started then abort commit
                    if !same_address(&current_value, original) {
                        return false;
                    }
                }
            }

            // second phase: write back and release

            // release the reads first because they are faster
            mem::drop(read_vec);


            for (var, value, mut lock) in write_vec {
                // commit value
                *lock = value;

                // unblock all threads waiting for it
                var.wake_all();
            }

            // commit succeded
            true
        })
    }


    /// run a STM computation atomically
    pub fn atomically(&self) -> T {
        // create a log guard for initializing and cleaning up
        // the log
        let _log_guard = LogGuard::new();

        // loop until success
        loop {
            use self::StmResult::*;

            // run the computation
            match self.intern_run() {
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
                        let blocking = log.vars.iter()
                            // take only read vars
                            .filter(|a| a.1.read.is_some())
                            // wait for all
                            .inspect(|a| {
                                a.0.wait(ctrl.clone());
                            })
                            // check if all still contain the same data
                            .all(|(ref var, value)| {
                                let guard = var.value.read().unwrap();
                                let newval = &*guard;
                                let oldval = value.read.as_ref().unwrap();
                                same_address(oldval, &newval)
                            });

                        // if no var has changed then block
                        if blocking { 
                            // propably wait until one var has changed
                            ctrl.wait();
                        }

                        // let others know that ctrl is dead
                        // it does not matter if we set too many
                        // to dead since it may slightly reduce performance
                        // but not break the semantics
                        for (var, value) in &log.vars {
                            if value.read.is_some() {
                                var.set_dead();
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


    /*
     * when the first computation fails, immediately rerun it without
     * trying the second one since 'or' provides an alternative to a
     * blocked computation but not for cases when a variable has changed
     * before finishing the computation
     */

    /// if one of both computations fails with a call to retry
    /// then run the other one
    ///
    /// if both call retry then the thread will block until any
    /// of the vars that were read in one of the both branches changes
    pub fn or(self, other: STM<'a, T>) -> STM<'a, T> {
        let func = move || {
            use self::StmResult::*;

            // create a backup of the log
            let backup = with_log(|log| log.clone());

            // run the first computation
            let s = self.intern_run();
            
            match s {
                // return success and failure
                a@Success(_)    => a,
                Failure         => Failure,
                // run other on retry
                Retry           => {
                    // use backup of log
                    let old_log = with_log(|log| mem::replace(log, backup));
                    // run other
                    let o = other.intern_run();

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

    /// run the first and afterwards the second one
    ///
    /// `first.and(second)` is equal to
    ///
    /// ```ignore
    /// stm!({
    ///     stm_call!(first);
    ///     stm_call!(second)
    /// });
    pub fn and<R: 'a>(self, other: STM<'a, R>) -> STM<'a, R> {
        STM::new(move || StmResult::Success({
            stm_call!(self);
            stm_call!(other)
        }))
    }

    /// run the first and then applies the return value to the
    /// function `f` which returns a STM-Block that is then executed
    ///
    /// `first.and_then(second)` is equal to
    ///
    /// ```ignore
    /// stm!({
    ///     let x = stm_call!(first);
    ///     stm_call!(second(x))
    /// });
    pub fn and_then<F: 'a, R: 'a>(self, f: F) -> STM<'a, R>
        where   F: Fn(T) -> STM<'a, R>,
    {
        STM::new(move || StmResult::Success({
            let x = stm_call!(self);
            stm_call!(f(x))
        }))
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
    let _guard = LogGuard::new();
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

    assert_eq!(write.read_atomic(), 0);
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

    assert_eq!(write.read_atomic(), 42);
}


