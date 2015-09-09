use std::collections::{BTreeSet, BTreeMap};
use std::collections::btree_map::Entry::*;
use std::any::Any;
use std::rc::Rc;
use std::sync::{Arc};

use super::var::{Var, VarControlBlock};


#[derive(Clone)]
pub struct LogVar {
    /// if read contains the value that was read
    pub read: Option<Arc<Any>>,

    /// if written to contains the last value that was written
    pub write: Option<Arc<Any>>,
}

impl LogVar {
    pub fn empty() -> LogVar {
        LogVar {
            read: None,
            write: None,
        }
    }

    pub fn new_read(val: Arc<Any>) -> LogVar {
        LogVar {
            read: Some(val),
            write: None,
        }
    }

    pub fn new_write(val: Arc<Any>) -> LogVar {
        LogVar {
            read: None,
            write: Some(val),
        }
    }

    pub fn get_val(&self) -> Arc<Any> {
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
    pub vars: BTreeMap<Arc<VarControlBlock>, LogVar>
}

impl Log {
    /// create a new Log
    pub fn new() -> Log {
        Log {
            vars: BTreeMap::new()
        }
    }

    fn downcast<T: Any+Clone>(var: Arc<Any>) -> T {
        let value = var.downcast_ref::<T>();
        value.expect("Vars with different types and same address").clone()
    }

    /// read a variable and return the value
    ///
    /// this is not always consistent with the current value of the var but may
    /// be an outdated or written but not commited value
    pub fn read_var<T: Send+Sync+Any+Clone>(&mut self, var: &Var<T>) -> T {
        // check if the same var was written before
        let value = match self.vars.entry(var.control_block().clone()) {
            Occupied(entry)  => {
                entry.get().get_val()
            }
            Vacant(entry)    => {
                let value = var.read_ref();
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
        let boxed = Arc::new(value) as Arc<Any>;

        self.vars.entry(var.control_block().clone())
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
        for (k,v) in other.vars {
            // if read then insert
            if v.read.is_some() {
                self.vars.insert(k.clone(), v.clone());
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
fn testRead() {
    let mut log = Log::new();
    let var = Var::new(vec![1,2,3,4]);

    // the variable can be read
    assert_eq!(&*log.read_var(&var), &[1,2,3,4]);
}

#[test]
fn testWriteRead() {
    let mut log = Log::new();
    let var = Var::new(vec![1,2]);

    log.write_var(&var, vec![1,2,3,4]);
    // consecutive reads get the updated version
    assert_eq!(log.read_var(&var), [1,2,3,4]);

    // the original value is still preserved
    assert_eq!(var.read_immediate(), [1,2]);
}

