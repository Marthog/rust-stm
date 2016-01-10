// Copyright 2015 Gunnar Bergmann
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

use super::var::{Var, VarControlBlock};


/// LogVar is used by `Log` to track which `Var` was either read or written
#[derive(Clone)]
pub struct LogVar {
    /// if read contains the value that was read
    pub read: Option<Arc<Any+Send+Sync>>,

    /// if written to contains the last value that was written
    pub write: Option<Arc<Any+Send+Sync>>,
}

impl LogVar {
    /// create ab empty LogVar
    ///
    /// be carefully because most code expects either `read` or `write` to be `Some`
    pub fn empty() -> LogVar {
        LogVar {
            read: None,
            write: None,
        }
    }

    /// create a new var that has been read
    pub fn new_read(val: Arc<Any+Send+Sync>) -> LogVar {
       LogVar {
            read: Some(val),
            write: None,
        }
    }

    /// get the value
    ///
    /// if the var was written to it is the value inside of write
    /// else the one in read
    pub fn get_val(&self) -> Arc<Any+Send+Sync> {
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
#[derive(Clone)]
pub struct Log {
    /// map of all vars that map the `VarControlBlock` of a var to a LogVar
    ///
    /// the `VarControlBlock` is unique because it uses it's address for comparing
    ///
    /// the logs need to be accessed in a order because otherwise 
    pub vars: BTreeMap<Arc<VarControlBlock>, LogVar>
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
                    LogVar::new_read(value.clone())
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
            .or_insert_with(LogVar::empty)
            .write = Some(boxed);
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
            if value.read.is_some() {
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

