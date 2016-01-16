// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::collections::{BTreeMap};
use std::collections::btree_map::Entry::*;
use std::any::Any;
use std::sync::{Arc};
use std::mem;

use super::var::{Var, VarControlBlock};
use super::stm::StmControlBlock;

/// LogVar is used by `Log` to track which `Var` was either read or written
#[derive(Clone)]
enum LogVar {
    Read(Arc<Any+Send+Sync>),
    Write(Arc<Any+Send+Sync>),
}

impl LogVar {
    pub fn read(&self) -> Option<Arc<Any+Send+Sync>> {
        use self::LogVar::*;
        match *self {
            Read(ref val) => Some(val.clone()),
            Write(_) => None,
        }
    }

    /// get the value
    ///
    /// if the var was written to it is the value inside of write
    /// else the one in read
    pub fn get_val(&self) -> Arc<Any+Send+Sync> {
        use self::LogVar::*;
        match *self {
            Read(ref val) => val,
            Write(ref val) => val,
        }.clone()
    }
}



/// Log used by STM the track all the read and written variables
///
/// used for checking vars to ensure atomicity
#[derive(Clone)]
pub struct Log {
    /// map of all vars that map the `VarControlBlock` of a var to a LogVar
    ///
    /// the `VarControlBlock` is unique because it uses it's address for comparing
    ///
    /// the logs need to be accessed in a order because otherwise 
    vars: BTreeMap<Arc<VarControlBlock>, LogVar>
}

impl Log {
    /// create a new Log
    ///
    /// normally you don't need to call this directly because the log
    /// is created as a thread-local global variable
    pub fn new() -> Log {
        Log {
            vars: BTreeMap::new()
        }
    }

    /// perform a downcast on a var
    fn downcast<T: Any+Clone>(var: Arc<Any>) -> T {
        var.downcast_ref::<T>()
            .expect("Vars with different types and same address")
            .clone()
    }

    /// read a variable and return the value
    ///
    /// this is not always consistent with the current value of the var but may
    /// be an outdated or written but not commited value
    pub fn read_var<T: Send+Sync+Any+Clone>(&mut self, var: &Var<T>) -> T {
        let ctrl = var.control_block().clone();
        // check if the same var was written before
        let value = match self.vars.entry(ctrl) {
            // if the variable has been accessed before than load that value
            Occupied(entry)  => {
                entry.get().get_val()
            }
            // else load the variable statically
            Vacant(entry)    => {
                // read the value from the var
                let value = var.read_ref_atomic();

                // store in in an entry
                entry.insert(
                    LogVar::Read(value.clone())
                );
                value
            }
        };
        Log::downcast(value)
    }

    /// write a variable
    ///
    /// does not immediately change the value but atomically
    /// commit all writes at the end of the computation
    pub fn write_var<T: Any+Send+Sync+Clone>(&mut self, var: &Var<T>, value: T) {
        // box the value
        let boxed = Arc::new(value);

        let ctrl = var.control_block().clone();
        self.vars
            .entry(ctrl)
            .or_insert_with(|| LogVar::Write(boxed));
    }

    /// write the log back to the variables
    ///
    /// return true for success and false if a read var has changed
    pub fn commit(&mut self) -> bool {
        // use two phase locking for safely writing data back to the vars

        // first phase: acquire locks
        // check for correctness of the values and perform
        // an early return if something is not consistent

        // created arrays for storing the locks

        // vector of locks
        let mut read_vec = Vec::new();

        // vector of tuple (variable, value, lock)
        let mut write_vec = Vec::new();

        for (var, value) in &self.vars {
            // lock the variable and read the value
            let current_value =
                match *value {
                LogVar::Write(ref written) => {
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
            if let LogVar::Read(ref original) = *value {
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
    }

    /// Wait for changes
    pub fn wait(&mut self) {
        // create control block for waiting
        let ctrl = Arc::new(StmControlBlock::new());

        let blocking = self.vars.iter()
            // take only read vars
            .filter_map(|(var, value)| value.read().map(|value| (var, value)))
            // wait for all
            .inspect(|&(ref var, _)| var.wait(ctrl.clone()))
            // check if all still contain the same data
            .all(|(ref var, ref value)| {
                let guard = var.value.read().unwrap();
                let newval = &*guard;
                same_address(value, &newval)
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
        for (var, value) in &self.vars {
            if let LogVar::Read(_) = *value {
                var.set_dead();
            }
        }
    }

    /// both logs called `retry`
    ///
    /// combine them into a single log to allow waiting for all reads
    ///
    /// needed for `STM::or`
    pub fn combine_after_retry(&mut self, other: Log) {
        // combine the reads
        for (var, value) in other.vars {
            // if read then insert
            if let LogVar::Read(_) = value {
                self.vars.insert(
                    var.clone(),
                    value.clone()
                );
            }
        }
    }

    /// clear the log's data
    ///
    /// this should be used before redoing a computation but
    /// nowhere else
    pub fn clear(&mut self) {
        self.vars.clear();
    }
}

fn same_address<T: ?Sized>(a: &Arc<T>, b: &Arc<T>) -> bool {
    arc_to_address(a) == arc_to_address(b)
}

fn arc_to_address<T: ?Sized>(arc: &Arc<T>) -> usize {
    &**arc as *const T as *const u32 as usize
}


#[test]
fn test_read() {
    let mut log = Log::new();
    let var = Var::new(vec![1,2,3,4]);

    // the variable can be read
    assert_eq!(&*log.read_var(&var), &[1,2,3,4]);
}

#[test]
fn test_write_read() {
    let mut log = Log::new();
    let var = Var::new(vec![1,2]);

    log.write_var(&var, vec![1,2,3,4]);
    // consecutive reads get the updated version
    assert_eq!(log.read_var(&var), [1,2,3,4]);

    // the original value is still preserved
    assert_eq!(var.read_atomic(), [1,2]);
}

