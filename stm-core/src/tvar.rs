// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

use std::sync::{Arc, Weak};
use parking_lot::{Mutex, RwLock};
use std::mem;
use std::sync::atomic::{self, AtomicUsize};
use std::cmp;
use std::any::Any;
use std::marker::PhantomData;
use std::fmt::{Debug, self};

use super::result::*;
use super::transaction::control_block::ControlBlock;
use super::Transaction;

/// `VarControlBlock` contains all the useful data for a `Var` while beeing the same type.
///
/// The control block is accessed from other threads directly whereas `Var`
/// is just a typesafe wrapper around it.
pub struct VarControlBlock {
    /// `waiting_threads` is a list of all waiting threads protected by a mutex.
    waiting_threads: Mutex<Vec<Weak<ControlBlock>>>,

    /// `dead_threads` is a counter for all dead threads.
    ///
    /// When there are many dead threads waiting for a change, but
    /// nobody changes the value, then an automatic collection is
    /// performed.
    dead_threads: AtomicUsize,

    /// The inner value of the Var.
    ///
    /// It can be shared through a Arc without copying it too often.
    ///
    /// The Arc is also used by the threads to detect changes.
    /// The value in it should not be changed or locked because
    /// that may cause multiple threads to block unforeseen as well as
    /// causing deadlocks.
    ///
    /// The shared reference is protected by a `RWLock` so that multiple
    /// threads can safely block it. This ensures consistency, without
    /// preventing other threads from accessing the values.
    ///
    /// Starvation may occur, if one thread wants to write-lock but others
    /// keep holding read-locks.
    pub value: RwLock<Arc<dyn Any + Send + Sync>>,
}


impl VarControlBlock {
    /// create a new empty `VarControlBlock`
    pub fn new<T>(val: T) -> Arc<VarControlBlock>
        where T: Any + Sync + Send
    {
        let ctrl = VarControlBlock {
            waiting_threads: Mutex::new(Vec::new()),
            dead_threads: AtomicUsize::new(0),
            value: RwLock::new(Arc::new(val)),
        };
        Arc::new(ctrl)
    }

    /// Wake all threads that are waiting for this block.
    pub fn wake_all(&self) {
        // Atomically take all waiting threads from the value.
        let threads = {
            let mut guard = self.waiting_threads.lock();
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
    pub fn wait(&self, thread: &Arc<ControlBlock>) {
        let mut guard = self.waiting_threads.lock();

        guard.push(Arc::downgrade(thread));
    }

    /// Mark another `StmControlBlock` as dead.
    ///
    /// If the count of dead control blocks is too high,
    /// perform a cleanup.
    /// This prevents masses of old `StmControlBlock` to
    /// pile up when a variable is often read but rarely written.
    pub fn set_dead(&self) {
        // Increase by one.
        let deads = self.dead_threads.fetch_add(1, atomic::Ordering::Relaxed);

        // If there are too many then cleanup.

        // There is a potential data race that may occure when
        // one thread reads the number and then operates on
        // outdated data, but no serious mistakes may happen.
        if deads >= 64 {
            let mut guard = self.waiting_threads.lock();
            self.dead_threads.store(0, atomic::Ordering::SeqCst);

            // Remove all dead ones. Possibly free up the memory.
            guard.retain(|t| t.upgrade().is_some());
        }
    }

    fn get_address(&self) -> usize {
        self as *const VarControlBlock as usize
    }
}


// Implement some operators so that VarControlBlocks can be sorted.

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
#[derive(Clone)]
pub struct TVar<T> {
    /// The control block is the inner of the variable.
    /// 
    /// The rest of `TVar` is just the typesafe interface.
    control_block: Arc<VarControlBlock>,

    /// This marker is needed so that the variable can be used in a typesafe
    /// manner.
    _marker: PhantomData<T>,
}

impl<T> TVar<T>
    where T: Any + Sync + Send + Clone
{
    /// Create a new `TVar`.
    pub fn new(val: T) -> TVar<T> {
        TVar {
            control_block: VarControlBlock::new(val),
            _marker: PhantomData,
        }
    }

    /// `read_atomic` reads a value atomically, without starting a transaction.
    ///
    /// It is semantically equivalent to 
    ///
    /// ```
    /// # use stm_core::*;
    ///
    /// let var = TVar::new(0);
    /// atomically(|trans| var.read(trans));
    /// ```
    ///
    /// but more efficient.
    ///
    /// `read_atomic` returns a clone of the value.
    pub fn read_atomic(&self) -> T {
        let val = self.read_ref_atomic();

        (&*val as &dyn Any)
            .downcast_ref::<T>()
            .expect("wrong type in Var<T>")
            .clone()
    }

    /// Read a value atomically but return a reference.
    ///
    /// This is mostly used internally, but can be useful in
    /// some cases, because `read_atomic` clones the
    /// inner value, which may be expensive.
    pub fn read_ref_atomic(&self) -> Arc<dyn Any + Send + Sync> {
        self.control_block
            .value
            .read()
            .clone()
    }

    /// The normal way to access a var.
    ///
    /// It is equivalent to `transaction.read(&var)`, but more
    /// convenient.
    pub fn read(&self, transaction: &mut Transaction) -> StmResult<T> {
        transaction.read(self)
    }

    /// The normal way to write a var.
    ///
    /// It is equivalent to `transaction.write(&var, value)`, but more
    /// convenient.
    pub fn write(&self, transaction: &mut Transaction, value: T) -> StmResult<()> {
        transaction.write(self, value)
    }

    /// Modify the content of a `TVar` with the function f.
    ///
    /// ```
    /// # use stm_core::*;
    ///
    ///
    /// let var = TVar::new(21);
    /// atomically(|trans| 
    ///     var.modify(trans, |x| x*2)
    /// );
    ///
    /// assert_eq!(var.read_atomic(), 42);
    /// ```
    pub fn modify<F>(&self, transaction: &mut Transaction, f: F) -> StmResult<()> 
    where F: FnOnce(T) -> T
    {
        let old = self.read(transaction)?;
        self.write(transaction, f(old))
    }
    
    /// Replaces the value of a `TVar` with a new one, returning
    /// the old one.
    ///
    /// ```
    /// # use stm_core::*;
    ///
    /// let var = TVar::new(0);
    /// let x = atomically(|trans| 
    ///     var.replace(trans, 42)
    /// );
    ///
    /// assert_eq!(x, 0);
    /// assert_eq!(var.read_atomic(), 42);
    /// ```
    pub fn replace(&self, transaction: &mut Transaction, value: T) -> StmResult<T> {
        let old = self.read(transaction)?;
        self.write(transaction, value)?;
        Ok(old)
    }

    /// Check if two `TVar`s refer to the same position.
    pub fn ref_eq(this: &TVar<T>, other: &TVar<T>) -> bool {
        Arc::ptr_eq(&this.control_block, &other.control_block)
    }
    
    /// Access the control block of the var.
    ///
    /// Internal use only!
    pub fn control_block(&self) -> &Arc<VarControlBlock> {
        &self.control_block
    }
}

/// Debug output a struct.
///
/// Note that this function does not print the state atomically.
/// If another thread modifies the datastructure at the same time, it may print an inconsistent state.
/// If you need an accurate view, that reflects current thread-local state, you can implement it easily yourself with 
/// atomically.
///
/// Running `atomically` inside a running transaction panics. Therefore `fmt` uses
/// prints the state.
impl<T> Debug for TVar<T>
    where T: Any + Sync + Send + Clone,
          T: Debug,
{
    #[inline(never)]
    fn fmt(&self, f: &mut fmt::Formatter) -> Result<(), fmt::Error> {
        let x = self.read_atomic();
        f.debug_struct("TVar")
            .field("value", &x)
            .finish()
    }
}



#[test]
// Test if creating and reading a TVar works.
fn test_read_atomic() {
    let var = TVar::new(42);

    assert_eq!(42, var.read_atomic());
}


// More tests are in lib.rs.
