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

#[cfg(test)]
use super::Var;

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


    // when the first computation fails, immediately rerun it without
    // trying the second one since 'or' provides an alternative to a
    // blocked computation but not for cases when a variable has changed
    // before finishing the computation
    //

    /// if one of both computations fails with a call to retry
    /// then run the other one
    ///
    /// if both call retry then the thread will block until any
    /// of the vars that were read in one of the both branches changes
    pub fn or(self, other: STM<'a, T>) -> STM<'a, T> {
        STM::new(move |stm| {
            stm.or(&self, &other)
        })
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
        STM::new(move |trans| {
            stm_call!(trans, self);
            other.run(trans)
        })
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
        where F: Fn(T) -> STM<'a, R>
    {
        STM::new(move |log| {
            StmResult::Success({
                let x = stm_call!(log, self);
                stm_call!(log, f(x))
            })
        })
    }
}

#[test]
fn test_read_var() {
    let mut stm = Transaction::new();
    let var = Var::new(vec![1, 2]);
    let x = var.read(&mut stm);

    assert_eq!(x, [1, 2]);
}

#[test]
fn test_stm_simple() {
    let stm = STM::new(|_| StmResult::Success(42));
    let x = stm.atomically();
    assert_eq!(x, 42);
}


#[test]
fn test_stm_read() {
    let read = Var::new(42);

    let stm = STM::new(move |trans| {
        let r = read.read(trans);
        StmResult::Success(r)
    });
    let x = stm.atomically();

    assert_eq!(x, 42);
}

#[test]
fn test_stm_write() {
    let write = Var::new(42);

    let writecp = write.clone();
    let stm = STM::new(move |trans| {
        writecp.write(trans, 0);
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
    let stm = STM::new(move |trans| {
        let r = read.read(trans);
        writecp.write(trans, r);
        StmResult::Success(())
    });
    stm.atomically();

    assert_eq!(write.read_atomic(), 42);
}
