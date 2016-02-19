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

pub type ArcAny = Arc<Any + Send + Sync>;

/// LogVar is used by `Log` to track which `Var` was either read or written
#[derive(Clone)]
enum LogVar {
    /// Var has been read.
    Read(ArcAny),
    
    /// Var has been written and no dependency on the original exists.
    ///
    /// There is no need to check for consistency.
    Write(ArcAny),

    /// Var has been read first and then written.
    ///
    /// It needs to be checked for consistency.
    ReadWrite(ArcAny, ArcAny),

    /// Var has been read on blocked path.
    ///
    /// Don't check for consistency, but block on Var,
    /// so that the threat wakes up when the first path
    /// has been unlocked.
    ReadObsolete(ArcAny),

    /// Var has been read on blocked path and then written to.
    ///
    /// Don't check for consistency, but block on Var,
    /// so that the threat wakes up when the first path
    /// has been unlocked.
    ReadObsoleteWrite(ArcAny, ArcAny)
}


impl LogVar {
    /// read a value and potentially upgrade the state
    pub fn read(&mut self) -> ArcAny {
        use self::LogVar::*;
        let this = self.clone();

        match this {
            Read(v) | Write(v) | ReadWrite(_,v) | ReadObsoleteWrite(_,v)     => v,
            ReadObsolete(v)           => {
                *self = Read(v.clone());
                v
            }
        }
    }
    
    /// write a value and potetially upgrade the state
    pub fn write(&mut self, w: ArcAny)
    {
        use self::LogVar::*;
        // TODO: replace with unsafe
        let this = self.clone();
        *self = match this {
            Write(_)    => Write(w),
            ReadObsolete(r) | ReadObsoleteWrite(r, _)
                => ReadObsoleteWrite(r, w),
            Read(r) | ReadWrite(r, _)
                => ReadWrite(r, w),
        };
    }

    /// turn this into an obsolete version
    pub fn obsolete(self) -> Option<LogVar>
    {
        use self::LogVar::*;
        self.into_read_value()
            .map(|a| ReadObsolete(a))
    }

    pub fn into_read_value(self) -> Option<ArcAny> {
        use self::LogVar::*;
        match self {
            // Throw away the reads and set the writes to obsolete.
            Read(v) | ReadWrite(v,_) | ReadObsolete(v) | ReadObsoleteWrite(v,_)
                => Some(v),
            Write(_)    => None,
        }
    }
}

pub fn atomically<F, T>(f: F) -> T
    where F: Fn(&mut Transaction) -> StmResult<T>,
{
        // create a log guard for initializing and cleaning up
        // the log
        let mut trans = Transaction::new();

        // loop until success
        loop {
            use StmResult::*;
            let t = f(&mut trans);

            // run the computation
            match t {
                // on success exit loop
                Success(t) => {
                    if trans.log_writeback() {
                        return t;
                    }
                }

                // on failure rerun immediately
                Failure => (),

                // on retry wait for changes
                Retry => {
                    trans.wait_for_change();
                }
            }

            // clear log before retrying computation
            trans.clear();
        }
}

/// Log used by STM the track all the read and written variables
///
/// used for checking vars to ensure atomicity
pub struct Transaction<'this> {
    /// map of all vars that map the `VarControlBlock` of a var to a LogVar

    /// the `VarControlBlock` is unique because it uses it's address for comparing
    ///
    /// the logs need to be accessed in a order to prevend dead-locks on locking
    vars: BTreeMap<&'this VarControlBlock, LogVar>,
}

impl<'this> Transaction<'this> {
    pub fn with<F, T>(f: &'this F) -> T
    where F: Fn(&mut Transaction) -> StmResult<T>,
    {
        // create a log guard for initializing and cleaning up
        // the log
        let mut trans = Transaction::new();

        // loop until success
        loop {
            use StmResult::*;
            let t = (*f)(&mut trans);

            // run the computation
            match t {
                // on success exit loop
                Success(t) => {
                    if trans.log_writeback() {
                        return t;
                    }
                }

                // on failure rerun immediately
                Failure => (),

                // on retry wait for changes
                Retry => {
                    trans.wait_for_change();
                }
            }

            // clear log before retrying computation
            trans.clear();
        }
    }

    /// create a new Log
    ///
    /// normally you don't need to call this directly because the log
    /// is created as a thread-local global variable
    pub fn new() -> Transaction<'this> {
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
    pub fn read<T: Send + Sync + Any + Clone>(&mut self, var: &'this Var<T>) -> T {
        let ctrl = var.control_block();
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
        Transaction::downcast(value)
    }

    /// write a variable
    ///
    /// does not immediately change the value but atomically
    /// commit all writes at the end of the computation
    pub fn write<T: Any + Send + Sync + Clone>(&mut self, var: &'this Var<T>, value: T) {
        // box the value
        let boxed = Arc::new(value);

        let ctrl = var.control_block();
        match self.vars.entry(ctrl) {
            Occupied(mut entry)     => entry.get_mut().write(boxed),
            Vacant(entry)       => { entry.insert(LogVar::Write(boxed)); }
        }
    }

    pub fn or<'a, T>(&mut self, 
                     first: &'this STM<'a, T>, 
                     second: &'this STM<'a, T>) 
        -> StmResult<T> {
        use super::StmResult::*;

        // Create a backup of the log.
        //
        // TODO: Get rid of the overhead for copying everything.
        //       Maybe have a stack of logs.
        let mut copy = Transaction {
            vars: self.vars.clone()
        };

        // Run the first computation.
        let f = first.run(self);

        match f {
            // Return success and failure directly
            s@Success(_) => s,
            Failure => Failure,

            // Run other on manual retry call.
            Retry => {
                // swap, so that self is the current run
                mem::swap(self, &mut copy);

                // Run other action.
                let s = second.run(self);

                // If both called retry then exit.
                match s {
                    Failure         => Failure,
                    s@Success(_) | s@Retry => {
                        self.combine(copy);
                        s
                    }
                }
            }
        }
    }

    /// Combine two logs into a single log to allow waiting for all reads.
    fn combine(&mut self, other: Transaction<'this>) {
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
    pub fn clear(&mut self) {
        self.vars.clear();
    }

    pub fn wait_for_change(&mut self) {
        // create control block for waiting
        let ctrl = Arc::new(StmControlBlock::new());

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

    /// write the log back to the variables
    ///
    /// return true for success and false if a read var has changed
    pub fn log_writeback(&mut self) -> bool {
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
            use self::LogVar::*;
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

#[test]
fn test_read() {
    let var = Var::new(vec![1, 2, 3, 4]);
    let mut log = Transaction::new();

    // the variable can be read
    assert_eq!(&*log.read(&var), &[1, 2, 3, 4]);
}

#[test]
fn test_write_read() {
    let var = Var::new(vec![1, 2]);
    let mut log = Transaction::new();

    log.write(&var, vec![1, 2, 3, 4]);
    // consecutive reads get the updated version
    assert_eq!(log.read(&var), [1, 2, 3, 4]);

    // the original value is still preserved
    assert_eq!(var.read_atomic(), [1, 2]);
}
