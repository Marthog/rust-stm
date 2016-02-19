// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


use std::sync::{Mutex, Condvar};
use std::sync::atomic::{AtomicBool, Ordering};

use super::Transaction;
use super::transaction::atomically;

#[cfg(test)]
use super::Var;

/// a control block for a currently running STM instance
///
/// STM blocks on all read variables if retry was called
/// this control block is used to let the vars inform the STM instance
///
/// Be careful when using this, because you can easily create deadlocks.
pub struct StmControlBlock {
    // a simple binary semaphore to unblock
    /// boolean storing true, if the ControlBlock is still blocked.
    /// It could be put in the mutex, but that may
    /// block a thread, that is currently releasing
    /// multiple variables on writing that value.
    blocked: AtomicBool,

    /// a lock needed for the condition variable
    lock: Mutex<()>,

    /// condition variable that is used for pausing and
    /// waking the thread
    wait_cvar: Condvar,
}


impl StmControlBlock {
    /// create a new StmControlBlock
    pub fn new() -> StmControlBlock {
        StmControlBlock {
            blocked: AtomicBool::new(true),
            lock: Mutex::new(()),
            wait_cvar: Condvar::new(),
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

    /// Block until one variable has changed.
    ///
    /// `wait` may immediately return.
    ///
    /// `wait` needs to be called by the STM instance itself.
    pub fn wait(&self) {
        let mut blocked = self.blocked.load(Ordering::SeqCst);
        let mut lock = self.lock.lock().unwrap();
        while blocked {
            lock = self.wait_cvar.wait(lock).unwrap();
            blocked = self.blocked.load(Ordering::SeqCst);
        }
    }
}


/// StmResult is a result of a single step of a STM calculation.
///
/// It informs of success or the type of failure.
pub enum StmResult<T> {
    /// The call succeeded.
    Success(T),

    /// The call failed, because a variable, the computation
    /// depends on, has changed.
    Failure,

    /// `retry` was called.
    ///
    /// It may block until at least one read variable has changed.
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
/// let infinite_retry = stm!(trans => {
///     stm_try!(retry());
/// });
/// # }
/// ```

pub fn retry<T>() -> StmResult<T> {
    StmResult::Retry
}

/// type synonym for the inner of a STM calculation
type StmFunction<'a, T> = Fn(&mut Transaction) -> StmResult<T> + 'a;

/// class representing a STM computation
pub struct STM<'a, T> {
    /// STM uses a boxed closure internally
    intern: Box<Fn(&mut Transaction) -> StmResult<T> + 'a>,
}

impl<'a, T: 'a> STM<'a, T> {
    /// create a new STM calculation from a closure
    pub fn new<F>(func: F) -> STM<'a, T>
        where F: Fn(&mut Transaction) -> StmResult<T> + 'a
    {
        STM { intern: Box::new(func) as Box<StmFunction<'a, T>> }
    }

    /// run a computation and return the result
    ///
    /// internal use only. Prefer atomically because it sets up
    /// the log and retry the computation until it has succeeded
    ///
    /// internal use only
    pub fn run(&self, log: &mut Transaction) -> StmResult<T> {
        // can't call directly because rust assumes
        // self.intern() to be a method call
        (self.intern)(log)
    }

    /// run a STM computation atomically
    pub fn atomically(&self) -> T {
        // create a log guard for initializing and cleaning up
        // the log
        let mut log = Transaction::new();

        // loop until success
        loop {
            use self::StmResult::*;

            // run the computation
            match self.run(&mut log) {
                // on success exit loop
                Success(t) => {
                    if log.log_writeback() {
                        return t;
                    }
                }

                // on failure rerun immediately
                Failure => (),

                // on retry wait for changes
                Retry => {
                    log.wait_for_change();
                }
            }

            // clear log before retrying computation
            log.clear();
        }
    }
}

#[test]
fn test_read_var() {
    let var = Var::new(vec![1, 2]);
    let mut stm = Transaction::new();
    let x = var.read(&mut stm);

    assert_eq!(x, [1, 2]);
}

#[test]
fn test_stm_simple() {
    let x = Transaction::with(&|_| StmResult::Success(42));
    assert_eq!(x, 42);
}

#[test]
fn test_stm_read() {
    // PROBLEM:
    //     read is local.
    //     read needs to outlive stm.
    //     read needs to outlive trans, because of read.read.
    //     trans has undetermined livetime of at least function call.
    //     -> can't proof inside of stm, that trans outlives read
    let read = Var::new(42);
    let stm = |trans| {
        let r = read.read(trans);
        StmResult::Success(r)
    };
    let x = atomically(&stm);

    assert_eq!(x, 42);
}

/*
#[test]
fn test_stm_write() {
    let write = Var::new(42);

    let stm = STM::new(|trans| {
        write.write(trans, 0);
        StmResult::Success(())
    });
    let _ = stm.atomically();

    assert_eq!(write.read_atomic(), 0);
}
*/

/*
#[test]
fn test_stm_copy() {
    let read = Var::new(42);
    let write = Var::new(0);

    let stm = STM::new(|trans| {
        let r = read.read(trans);
        write.write(trans, r);
        StmResult::Success(())
    });
    stm.atomically();

    assert_eq!(write.read_atomic(), 42);
}
*/
