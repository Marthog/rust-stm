// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

pub mod control_block;
pub mod log_var;

use std::collections::BTreeMap;
use std::collections::btree_map::Entry::*;
use std::mem;
use std::sync::{Arc};
use std::any::Any;

use self::log_var::LogVar;
use self::log_var::LogVar::*;
use self::control_block::ControlBlock;
use super::var::{TVar, VarControlBlock};
use super::result::*;
use super::result::StmError::*;


/// Transaction tracks all the read and written variables.
///
/// It is used for checking vars, to ensure atomicity.
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
    fn new() -> Transaction {
        Transaction { vars: BTreeMap::new() }
    }

    /// Run a function with a transaction.
    ///
    /// It is equivalent to `atomically`.
    pub fn with<T, F>(f: F) -> T 
    where F: Fn(&mut Transaction) -> StmResult<T>,
    {
        // create a log guard for initializing and cleaning up
        // the log
        let mut transaction = Transaction::new();

        // loop until success
        loop {
            // run the computation
            match f(&mut transaction) {
                // on success exit loop
                Ok(t) => {
                    if transaction.commit() {
                        return t;
                    }
                }

                // on failure rerun immediately
                Err(Failure) => { }

                // on retry wait for changes
                Err(Retry) => {
                    transaction.wait_for_change();
                }
            }

            // clear log before retrying computation
            transaction.clear();
        }
    }

    /// Perform a downcast on a var.
    fn downcast<T: Any + Clone>(var: Arc<Any>) -> T {
        var.downcast_ref::<T>()
           .expect("Vars with different types and same address")
           .clone()
    }

    /// Read a variable and return the value.
    ///
    /// this is not always consistent with the current value of the var but may
    /// be an outdated or written but not commited value
    ///
    /// The used code should be capable of handling inconsistens states
    /// without running into infinite loops.
    /// Just the commit of wrong values is prevented.
    pub fn read<T: Send + Sync + Any + Clone>(&mut self, var: &TVar<T>) -> StmResult<T> {
        let ctrl = var.control_block().clone();
        // Check if the same var was written before.
        let value = match self.vars.entry(ctrl) {

            // If the variable has been accessed before, then load that value.
            Occupied(mut entry) => entry.get_mut().read(),

            // Else load the variable statically.
            Vacant(entry) => {
                // Read the value from the var.
                let value = var.read_ref_atomic();

                // Store in in an entry.
                entry.insert(LogVar::Read(value.clone()));
                value
            }
        };

        // For now always succeeds, but that may change later.
        Ok(Transaction::downcast(value))
    }

    /// Write a variable.
    ///
    /// `write` does not immediately change the value, but atomically
    /// commits all writes at the end of the computation.
    pub fn write<T: Any + Send + Sync + Clone>(&mut self, var: &TVar<T>, value: T) -> StmResult<()> {
        // box the value
        let boxed = Arc::new(value);

        // new control block
        let ctrl = var.control_block().clone();
        // update or create new entry
        match self.vars.entry(ctrl) {
            Occupied(mut entry)     => entry.get_mut().write(boxed),
            Vacant(entry)       => { entry.insert(LogVar::Write(boxed)); }
        }

        // For now always succeeds, but that may change later.
        Ok(())
    }

    /// Combine two calculations. When one blocks with `retry`, 
    /// run the other, but don't commit the changes in the first.
    /// It still blocks on all, so that you can wait for one of
    /// two alternatives.
    pub fn or<T, F1, F2>(&mut self, first: F1, second: F2) -> StmResult<T>
        where F1: Fn(&mut Transaction) -> StmResult<T>,
              F2: Fn(&mut Transaction) -> StmResult<T>,
    {

        // Create a backup of the log.
        let mut copy = Transaction {
            vars: self.vars.clone()
        };

        // Run the first computation.
        let f = first(self);

        match f {
            // Run other on manual retry call.
            Err(Retry)      => {
                // swap, so that self is the current run
                mem::swap(self, &mut copy);

                // Run other action.
                let s = second(self);

                // If both called retry then exit.
                match s {
                    Err(Failure)        => Err(Failure),
                    s => {
                        self.combine(copy);
                        s
                    }
                }
            }

            // Return success and failure directly
            x               => x,
        }
    }

    /// Combine two logs into a single log, to allow waiting for all reads.
    fn combine(&mut self, other: Transaction) {
        // combine reads
        for (var, value) in other.vars {
            // only insert new values
            if let Some(value) = value.obsolete() {
                self.vars.entry(var).or_insert(value);
            }
        }
    }

    /// Clear the log's data.
    ///
    /// This should be used before redoing a computation, but
    /// nowhere else.
    fn clear(&mut self) {
        self.vars.clear();
    }

    /// Wait for any variable to change,
    /// because the change may lead to a new calculation result.
    fn wait_for_change(&mut self) {
        // create control block for waiting
        let ctrl = Arc::new(ControlBlock::new());

        let vars = mem::replace(&mut self.vars, BTreeMap::new());
        let mut reads = Vec::new();
            
        let blocking = vars.into_iter()
            .filter_map(|(a, b)| {
                b.into_read_value()
                    .map(|b| (a, b))
            })
            // check if all still contain the same data
            .all(|(var, value)| {
                var.wait(&ctrl);
                let x = {
                    // Take read lock and read value.
                    let guard = var.value.read().unwrap();
                    same_address(&value, &guard)
                };
                reads.push(var);
                x
            });

        // If no var has changed, then block.
        if blocking {
            // propably wait until one var has changed
            ctrl.wait();
        }

        // Let others know that ctrl is dead.
        // It does not matter, if we set too many
        // to dead since it may slightly reduce performance
        // but not break the semantics.
        for var in &reads {
            var.set_dead();
        }
    }

    /// Write the log back to the variables.
    ///
    /// Return true for success and false, if a read var has changed
    fn commit(&mut self) -> bool {
        // Replace with new structure, so that we don't have to copy.
        let vars = mem::replace(&mut self.vars, BTreeMap::new());
        // Use two phase locking for safely writing data back to the vars.

        // First phase: acquire locks.
        // Check for correctness of the values and perform
        // an early return if something is not consistent.

        // Created arrays for storing the locks
        // vector of locks
        let mut read_vec = Vec::new();

        // vector of tuple (variable, value, lock)
        let mut write_vec = Vec::new();

        for (var, value) in &vars {
            // lock the variable and read the value

            match value {
                // We need to take a write lock.
                &Write(ref w) | &ReadObsoleteWrite(_,ref w)=> {
                    // take write lock
                    let lock = var.value.write().unwrap();
                    // add all data to the vector
                    write_vec.push((var, w, lock));
                }
                
                // We need to check for consistency and
                // take a write lock.
                &ReadWrite(ref original,ref w) => {
                    // take write lock
                    let lock = var.value.write().unwrap();

                    if !same_address(&lock, original) {
                        return false;
                    }
                    // add all data to the vector
                    write_vec.push((var, w, lock));
                }
                // Nothing to do. ReadObsolete is only needed for blocking, not
                // for consistency checks.
                &ReadObsolete(_) => { }
                // Take read lock and check for consistency.
                &Read(ref original) => {
                    // take a read lock
                    let lock = var.value.read().unwrap();

                    if !same_address(&lock, original) {
                        return false;
                    }

                    read_vec.push(lock);
                }
            }
        }

        // Second phase: write back and release

        // Release the reads first.
        mem::drop(read_vec);

        for (var, value, mut lock) in write_vec {
            // commit value
            *lock = value.clone();

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


/// Test same_address on a cloned Arc
#[test]
fn test_same_address_equal() {
    let t1 = Arc::new(42);
    let t2 = t1.clone();
    
    assert!(same_address(&t1, &t2));
}

/// Test same_address on differenc Arcs with same value
#[test]
fn test_same_address_different() {
    let t1 = Arc::new(42);
    let t2 = Arc::new(42);
    
    assert!(!same_address(&t1, &t2));
}

#[test]
fn test_read() {
    let mut log = Transaction::new();
    let var = TVar::new(vec![1, 2, 3, 4]);

    // the variable can be read
    assert_eq!(&*log.read(&var).unwrap(), &[1, 2, 3, 4]);
}

#[test]
fn test_write_read() {
    let mut log = Transaction::new();
    let var = TVar::new(vec![1, 2]);

    log.write(&var, vec![1, 2, 3, 4]).unwrap();

    // consecutive reads get the updated version
    assert_eq!(log.read(&var).unwrap(), [1, 2, 3, 4]);

    // the original value is still preserved
    assert_eq!(var.read_atomic(), [1, 2]);
}

#[test]
fn test_transaction_simple() {
    let x = Transaction::with(|_| Ok(42));
    assert_eq!(x, 42);
}

#[test]
fn test_transaction_read() {
    let read = TVar::new(42);

    let x = Transaction::with(|trans| {
        read.read(trans)
    });

    assert_eq!(x, 42);
}

#[test]
fn test_transaction_write() {
    let write = TVar::new(42);

    Transaction::with(|trans| {
        write.write(trans, 0)
    });

    assert_eq!(write.read_atomic(), 0);
}

#[test]
fn test_transaction_copy() {
    let read = TVar::new(42);
    let write = TVar::new(0);

    Transaction::with(|trans| {
        let r = try!(read.read(trans));
        write.write(trans, r)
    });

    assert_eq!(write.read_atomic(), 42);
}

