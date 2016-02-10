// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::collections::BTreeMap;
use std::collections::btree_map::Entry::*;
use std::any::Any;
use std::sync::Arc;
use std::mem;

use super::var::{Var, VarControlBlock};
use super::{STM, StmResult};
use super::stm::StmControlBlock;


/// LogVar is used by `Log` to track which `Var` was either read or written
#[derive(Clone)]
pub struct LogVar {
    /// if read contains the value that was read
    pub read: Option<Arc<Any + Send + Sync>>,

    /// if written to contains the last value that was written
    pub write: Option<Arc<Any + Send + Sync>>,
}

impl LogVar {
    /// create an empty LogVar
    ///
    /// be carefully because most code expects either `read` or `write` to be `Some`
    pub fn empty() -> LogVar {
        LogVar {
            read: None,
            write: None,
        }
    }

    /// create a new var that has been read
    pub fn new_read(val: Arc<Any + Send + Sync>) -> LogVar {
        LogVar {
            read: Some(val),
            write: None,
        }
    }

    /// get the value
    ///
    /// if the var was written to it is the value inside of write
    /// else the one in read
    pub fn get_val(&self) -> Arc<Any + Send + Sync> {
        if let Some(ref s) = self.write {
            s.clone()
        } else {
            self.read.as_ref().unwrap().clone()
        }
    }
}


/// Log used by STM the track all the read and written variables
///
/// used for checking vars to ensure atomicity
pub struct Transaction {
    /// map of all vars that map the `VarControlBlock` of a var to a LogVar

    /// the `VarControlBlock` is unique because it uses it's address for comparing
    ///
    /// the logs need to be accessed in a order to prevend dead-locks on locking
    vars: BTreeMap<Arc<VarControlBlock>, LogVar>,
}

impl Transaction {
    /// create a new Log
    ///
    /// normally you don't need to call this directly because the log
    /// is created as a thread-local global variable
    pub fn new() -> Transaction {
        Transaction { vars: BTreeMap::new() }
    }

    /// perform a downcast on a var
    fn downcast<T: Any + Clone>(var: Arc<Any>) -> T {
        var.downcast_ref::<T>()
           .expect("Vars with different types and same address")
           .clone()
    }

    /// read a variable and return the value
    ///
    /// this is not always consistent with the current value of the var but may
    /// be an outdated or written but not commited value
    pub fn read<T: Send + Sync + Any + Clone>(&mut self, var: &Var<T>) -> T {
        let ctrl = var.control_block().clone();
        // Check if the same var was written before.
        let value = match self.vars.entry(ctrl) {

            // If the variable has been accessed before, then load that value.
            Occupied(entry) => entry.get().get_val(),

            // Else load the variable statically.
            Vacant(entry) => {
                // Read the value from the var.
                let value = var.read_ref_atomic();

                // Store in in an entry.
                entry.insert(LogVar::new_read(value.clone()));
                value
            }
        };
        Transaction::downcast(value)
    }

    /// write a variable
    ///
    /// does not immediately change the value but atomically
    /// commit all writes at the end of the computation
    pub fn write<T: Any + Send + Sync + Clone>(&mut self, var: &Var<T>, value: T) {
        // box the value
        let boxed = Arc::new(value);

        let ctrl = var.control_block().clone();
        self.vars
            .entry(ctrl)
            .or_insert_with(LogVar::empty)
            .write = Some(boxed);
    }

    pub fn or<'a, T>(&mut self, first: &STM<'a, T>, second: &STM<'a, T>) -> StmResult<T> {
        use super::StmResult::*;

        // Create a backup of the log.
        let mut copy = Transaction {
            vars: self.vars.clone()
        };

        // Run the first computation.
        let f = first.intern_run(self);

        match f {
            // Return success and failure directly
            s@Success(_) => s,
            Failure => Failure,

            // Run other on manual retry call.
            Retry => {
                // Run other action.
                let s = second.intern_run(&mut copy);

                // If both called retry then exit.
                if let Retry = s {
                    // Combine both logs so that all reads are considered

                    // TODO: combine even on Success, so that we can block
                    // correctly if retry is called after the join, so that
                    // we wait on either of them to change.
                    //
                    // We still need to verify just the one path during commit.
                    self.combine_after_retry(copy);
                }
                // Return the result of the second action.
                s
            }
        }
    }


    /// Both logs called `retry`.
    ///
    /// Combine them into a single log to allow waiting for all reads.
    fn combine_after_retry(&mut self, other: Transaction) {
        // combine the reads
        for (var, value) in other.vars {
            // if read then insert
            if value.read.is_some() {
                self.vars.insert(var.clone(), value.clone());
            }
        }
    }

    /// Clear the log's data.
    ///
    /// This should be used before redoing a computation, but
    /// nowhere else.
    pub fn clear(&mut self) {
        self.vars.clear();
    }

    pub fn wait_for_change(&mut self) {
        // create control block for waiting
        let ctrl = Arc::new(StmControlBlock::new());

        let blocking = self.vars.iter()
            // take only read vars
            .filter(|a| a.1.read.is_some())
            // wait for all
            .inspect(|a| {
                a.0.wait(ctrl.clone());
            })
            // check if all still contain the same data
            .all(|(ref var, value)| {
                // Take read lock and read value.
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

        // Let others know that ctrl is dead.
        // It does not matter, if we set too many
        // to dead since it may slightly reduce performance
        // but not break the semantics.
        for (var, value) in &self.vars {
            if value.read.is_some() {
                var.set_dead();
            }
        }
    }

    /// write the log back to the variables
    ///
    /// return true for success and false if a read var has changed
    pub fn log_writeback(&mut self) -> bool {
        // Use two phase locking for safely writing data back to the vars.

        // First phase: acquire locks.
        // Check for correctness of the values and perform
        // an early return if something is not consistent.

        // Created arrays for storing the locks
        // vector of locks
        let mut read_vec = Vec::new();

        // vector of tuple (variable, value, lock)
        let mut write_vec = Vec::new();

        for (var, value) in &self.vars {
            // lock the variable and read the value
            let current_value;
            match value.write {
                Some(ref written) => {
                    // take write lock
                    let lock = var.value.write().unwrap();
                    // get the current value
                    current_value = lock.clone();
                    // add all data to the vector
                    write_vec.push((var, written.clone(), lock));
                }
                _ => {
                    // take a read lock
                    let lock = var.value.read().unwrap();
                    // take the current value
                    current_value = lock.clone();
                    read_vec.push(lock);
                }
            };

            // if the value was read then compare
            if let Some(ref original) = value.read {
                // If the current value is no longer that
                // consistent to the computation start, then abort commit.
                if !same_address(&current_value, original) {
                    return false;
                }
            }
        }

        // Second phase: write back and release

        // Release the reads first because they are faster
        mem::drop(read_vec);

        for (var, value, mut lock) in write_vec {
            // commit value
            *lock = value;

            // Unblock all threads waiting for it.
            var.wake_all();
        }

        // commit succeded
        true
    }

}


fn arc_to_address<T: ?Sized>(arc: &Arc<T>) -> usize {
    &**arc as *const T as *const u32 as usize
}

fn same_address<T: ?Sized>(a: &Arc<T>, b: &Arc<T>) -> bool {
    arc_to_address(a) == arc_to_address(b)
}

#[test]
fn test_read() {
    let mut log = Transaction::new();
    let var = Var::new(vec![1, 2, 3, 4]);

    // the variable can be read
    assert_eq!(&*log.read(&var), &[1, 2, 3, 4]);
}

#[test]
fn test_write_read() {
    let mut log = Transaction::new();
    let var = Var::new(vec![1, 2]);

    log.write(&var, vec![1, 2, 3, 4]);
    // consecutive reads get the updated version
    assert_eq!(log.read(&var), [1, 2, 3, 4]);

    // the original value is still preserved
    assert_eq!(var.read_atomic(), [1, 2]);
}
