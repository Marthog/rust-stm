// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::sync::{Mutex, Condvar};
use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(test)]
use super::super::test::{terminates, terminates_async};

// A control block for a currently running STM instance
///
/// STM blocks on all read variables if retry was called
/// this control block is used to let the vars inform the STM instance
///
/// Be careful when using this, because you can easily create deadlocks.
pub struct ControlBlock {
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


impl ControlBlock {
    /// create a new StmControlBlock
    pub fn new() -> ControlBlock {
        ControlBlock {
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


// TESTS

/// Test if ControlBlock correctly blocks on `wait`.
#[test]
fn test_blocked() {
    let ctrl = ControlBlock::new();
    // waiting should immediately finish
    assert!(!terminates(100, move || ctrl.wait()));
}

/// A ControlBlock does immediately return,
/// when it was set to changed before calling waiting.
///
/// This can occur, when a variable changes, while the
/// transaction is registered on other variables.
#[test]
fn test_wait_after_change() {
    let ctrl = ControlBlock::new();
    // set to changed
    ctrl.set_changed();
    // waiting should immediately finish
    assert!(terminates(50, move || ctrl.wait()));
}

/// Test calling `set_changed` multiple times.
#[test]
fn test_wait_after_multiple_changes() {
    let ctrl = ControlBlock::new();
    // set to changed
    ctrl.set_changed();
    ctrl.set_changed();
    ctrl.set_changed();
    ctrl.set_changed();

    // waiting should immediately finish
    assert!(terminates(50, move || ctrl.wait()));
}


/// Perform a wakeup from another thread.
#[test]
fn test_wait_threaded_wakeup() {
    use std::sync::Arc;

    let ctrl = Arc::new(ControlBlock::new());
    let ctrl2 = ctrl.clone();
    let terminated = terminates_async(500,
                                move || ctrl.wait(),
                                move || ctrl2.set_changed());

    assert!(terminated);
}

