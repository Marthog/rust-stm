// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::sync::{Arc, Weak, Mutex, RwLock};
use std::mem;
use std::sync::atomic::{self, AtomicUsize};
use std::cmp;
use std::any::Any;
use std::marker::PhantomData;

use super::stm::{StmControlBlock};
use super::Transaction;

/// contains all the useful data for a Var while beeing the same type
///
/// The control block is accessed from other threads directly whereas `Var`
/// is just a typesafe wrapper around it
pub struct VarControlBlock {
    /// list of all waiting threads protected by a mutex
    waiting_threads: Mutex<Vec<Weak<StmControlBlock>>>,

    /// counter for all dead threads
    ///
    /// when there are many dead threads waiting for a change but
    /// nobody changes the value then an automatic collection is
    /// performed
    dead_threads: AtomicUsize,

    /// the inner value of the Var
    ///
    /// It can be shared through a Arc without copying it too often
    ///
    /// the Arc is also used by the threads to detect changes
    /// the value in it should not be changed or locked because
    /// that may cause multiple threads to block unforeseen as well as
    /// causing deadlocks
    ///
    /// the shared reference is protected by a `RWLock` so that multiple
    /// threads can safely block it for ensuring atomic commits without
    /// preventing other threads from accessing it
    ///
    /// starvation may occur when one thread wants to write-lock but others
    /// hold read-locks
    pub value: RwLock<Arc<Any + Send + Sync>>,
}

impl VarControlBlock {
    /// create a new empty `VarControlBlock`
    pub fn new<T>(val: T) -> VarControlBlock
        where T: Any + Sync + Send
    {
        VarControlBlock {
            waiting_threads: Mutex::new(Vec::new()),
            dead_threads: AtomicUsize::new(0),
            value: RwLock::new(Arc::new(val)),
        }
    }

    /// wake all threads that are waiting for the used var
    pub fn wake_all(&self) {
        // Atomically take all waiting threads from the value.
        let threads = {
            let mut guard = self.waiting_threads.lock().unwrap();
            let inner: &mut Vec<_> = &mut guard;
            mem::replace(inner, Vec::new())
        };

        // Take all, that are still alive.
        let threads = threads.iter()
            .filter_map(Weak::upgrade);

        // Release all the semaphores to start the thread.
        for thread in threads {
            // Inform thread that this var has changed.
            thread.set_changed();
        }
    }

    /// Add another thread, that waits for mutations of `self`.
    pub fn wait(&self, thread: &Arc<StmControlBlock>) {
        let mut guard = self.waiting_threads.lock().unwrap();

        guard.push(Arc::downgrade(thread));
    }

    /// mark another `StmControlBlock` as dead
    ///
    /// when the count of dead control blocks is too high
    /// then perform a cleanup
    ///
    /// this prevents masses of old `StmControlBlock` to
    /// pile up when a variable is often read but not written
    pub fn set_dead(&self) {
        // increase by one
        let deads = self.dead_threads.fetch_add(1, atomic::Ordering::Relaxed);

        // if there are too many then cleanup

        // there is a potential data race that may occure when
        // one thread reads the number and then operates on
        // outdated data but that causes just unnecessary locks
        // to occur and nothing serious
        if deads >= 64 {
            let mut guard = self.waiting_threads.lock().unwrap();
            self.dead_threads.store(0, atomic::Ordering::SeqCst);

            // remove all dead ones possibly free up the memory
            guard.retain(|t| t.upgrade().is_some());
        }
    }

    fn get_address(&self) -> usize {
        self as *const VarControlBlock as usize
    }
}


// implement some operators so that VarControlBlocks can be sorted

impl PartialEq for VarControlBlock {
    fn eq(&self, other: &Self) -> bool {
        self.get_address() == other.get_address()
    }
}

impl Eq for VarControlBlock {}

impl Ord for VarControlBlock {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.get_address().cmp(&other.get_address())
    }
}

impl PartialOrd for VarControlBlock {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}



/// A variable that can be used in a STM-Block
pub struct Var<T> {
    /// the control block is the inner of the variable
    /// 
    /// the rest is just the typesafe interface
    control_block: VarControlBlock,
    /// this marker is needed so that the variable can be used in a threadsafe
    /// manner
    _marker: PhantomData<T>,
}

impl<T> Var<T>
    where T: Any + Sync + Send + Clone
{
    /// create a new var
    pub fn new(val: T) -> Var<T> {
        Var {
            control_block: VarControlBlock::new(val),
            _marker: PhantomData,
        }
    }

    /// read a value atomically
    ///
    /// this should be called from outside of stm and is faster
    /// than wrapping a read in STM but is not composable
    ///
    /// `read_atomic` returns a clone of the value.
    ///
    /// If the value contains a shared reference mutating it is
    /// a side effect which may break STM-semantics
    ///
    /// This is a faster alternative to 
    ///
    /// ```
    /// # #[macro_use] extern crate stm;
    /// # use stm::*;
    /// # fn main() {
    /// # let var = Var::new(0);
    /// stm!(trans => var.read(trans))
    ///     .atomically();
    /// # }
    /// ```
    ///
    pub fn read_atomic(&self) -> T {
        let val = self.read_ref_atomic();

        (&*val as &Any)
            .downcast_ref::<T>()
            .expect("wrong type in Var<T>")
            .clone()
    }

    /// read a value atomically but return a reference
    ///
    /// this is mostly used internally but can be useful in
    /// certain cases where the additional clone performed
    /// by read_atomic is not wanted
    pub fn read_ref_atomic(&self) -> Arc<Any + Send + Sync> {
        self.control_block
            .value
            .read()
            .unwrap()
            .clone()
    }

    /// the normal way to access a var
    ///
    /// it is used to read a var from inside of a STM-Block
    ///
    /// # Panics
    ///
    /// Panics when called from outside of a STM-Block
    ///
    pub fn read<'a>(&'a self, transaction: &mut Transaction<'a>) -> T {
        transaction.read(self)
    }

    /// the normal way to write a var
    ///
    /// it is used to write a var from inside of a STM-Block
    /// and does not immediately write but wait for commit
    ///
    /// # Panics
    ///
    /// Panics when called from outside of a STM-Block
    ///
    pub fn write<'a>(&'a self, transaction: &mut Transaction<'a>, value: T) {
        transaction.write(self, value);
    }
    
    /// wake all threads that are waiting for this value
    ///
    /// this is mostly used internally
    pub fn wake_all(&self) {
        self.control_block.wake_all();
    }

    /// access the control block of the var
    ///
    /// internal use only
    pub fn control_block<'a>(&'a self) -> &'a VarControlBlock {
        &self.control_block
    }
}

/*
/// test if a waiting and waking of threads works
#[test]
fn test_wait() {
    use std::thread;
    use std::sync::mpsc::channel;
    use std::time::Duration;

    // don't create a complete STM block
    let ctrl = Arc::new(StmControlBlock::new());

    let var = Var::new(vec![1, 2, 3, 4]);

    let (tx, rx) = channel();

    // add to list of blocked things
    var.control_block.wait(&ctrl);
    // wake me again
    var.wake_all();


    let ctrl2 = ctrl.clone();
    // there is no way to ensure that the semaphore has been unlocked
    let handle = thread::spawn(move || {
        // 300 ms should be enough, otherwise way to slow
        thread::sleep(Duration::from_millis(300));
        let err = rx.try_recv().is_err();
        ctrl2.set_changed();
        if err {
            panic!("semaphore not released before timeout");
        }
    });

    // get semaphore
    ctrl.wait();

    let _ = tx.send(());

    let _ = handle.join();
}
*/
