// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! This library implements [software transactional memory]
//! (https://en.wikipedia.org/wiki/Software_transactional_memory),
//! often abbreviated with STM.
//!
//! It is designed closely to haskells STM library. Read Simon Marlow's
//! [Parallel and Concurrent Programming in Haskell]
//! (http://chimera.labs.oreilly.com/books/1230000000929/ch10.html)
//! for more info. Especially the chapter about [Performance]
//! (http://chimera.labs.oreilly.com/books/1230000000929/ch10.html#sec_stm-cost)
//! is also important for using STM in rust.
//!
//! With locks the sequential composition of two 
//! two threadsafe actions is no longer threadsafe because
//! other threads may interfer in between of these actions.
//! Applying a third lock to protect both may lead to common sources of errors
//! like deadlocks or race conditions.
//!
//! Unlike locks Software transactional memory is composable.
//! It is typically implemented by writing all read and write
//! operations in a log. When the action has finished and
//! all the used `TVar`s are consistend, the writes are commited as
//! a single atomic operation.
//! Otherwise the computation repeats. This may lead to starvation,
//! but avoids common sources of bugs.
//!
//! Panicing within STM does not poison the `TVar`s. STM ensures consistency by
//! never committing on panic.
//!
//! # Usage
//!
//! You should only use the functions that are safe to use.
//!
//! Don't have side effects, except those provided by `TVar`.
//! Especially a mutexes or other blocking mechanisms inside of software transactional
//! memory are dangerous.
//!
//! You can run the top-level atomic operation by calling `atomically`.
//!
//!
//! ```
//! use stm::atomically;
//! atomically(|trans| {
//!     // some action
//!     // return value as `Result`, for example
//!     Ok(42)
//! });
//! ```
//!
//! Calls to `atomically` should not be nested.
//!
//! For running an atomic operation inside of another, pass a mutable reference to a `Transaction`
//! and call `try!` on the result or use `?`. You should not handle the error yourself, because it
//! breaks consistency.
//!
//! ```
//! use stm::{atomically, TVar};
//! let var = TVar::new(0);
//!
//! let x = atomically(|trans| {
//!     var.write(trans, 42)?; // Pass failure to parent.
//!     var.read(trans) // Return the value saved in var.
//! });
//!
//! println!("var = {}", x);
//!
//! ```
//!
//! # STM safety
//!
//! Software transactional memory is completely safe in the terms,
//! that rust considers safe. Still there are multiple rules that
//! you should obey when dealing with software transactional memory:
//!
//! * Don't run code with side effects, especially no IO-code,
//! because stm repeats the computation when it detects inconsistent state.
//! Return a closure if you have to.
//! * Don't handle the error types yourself, unless you absolutely know, what you
//! are doing. Use `Transaction::or`, to combine alternative paths. Always call `try!` or
//! `?` and never ignore a `StmResult`.
//! * Don't run `atomically` inside of another. `atomically` is designed to have side effects
//! and will therefore break stm's assumptions. Nested calls are detected at runtime and
//! handled with panic.
//! When you use STM in the inner of a function, then
//! express it in the public interface, by taking `&mut Transaction` as parameter and 
//! returning `StmResult<T>`. Callers can safely compose it into
//! larger blocks.
//! * Don't mix locks and transactions. Your code will easily deadlock or slow
//! down on unpredictably.
//! * Don't use inner mutability to change the content of a `TVar`.
//!
//! # Speed
//!
//! Generally keep your atomic blocks as small as possible, because
//! the more time you spend, the more likely it is, to collide with
//! other threads. For STM, reading `TVar`s is quite slow, because it
//! needs to look them up in the log every time.
//! Every used `TVar` increases the chance of collisions. Therefore you should
//! keep the amount of accessed variables as low as needed.
//!

mod transaction;
mod tvar;
mod result;

#[cfg(test)]
mod test;

pub use tvar::TVar;
pub use transaction::Transaction;
pub use transaction::TransactionControl;
pub use result::*;

/// call `retry`, to abort an operation. It takes another path of an
/// `Transaction::or` or blocks until any variable changes.
///
/// # Examples
///
/// ```no_run
/// use stm::*;
/// let infinite_retry: i32 = atomically(|_| retry());
/// ```
pub fn retry<T>() -> StmResult<T> {
    Err(StmError::Retry)
}

/// Run a function atomically by using Software Transactional Memory.
/// It calls to `Transaction::with` internally, but is more explicit.
pub fn atomically<T, F>(f: F) -> T
where F: Fn(&mut Transaction) -> StmResult<T>
{
    Transaction::with(f)
}

#[inline]
/// Unwrap `Option` or call retry if it is `None`.
///
/// # Example
///
/// ```
/// use stm::*;
///
/// let x = atomically(|_tx|
///     unwrap_or_retry(Some(42))
/// );
/// assert_eq!(x, 42);
/// ```
pub fn unwrap_or_retry<T>(option: Option<T>) 
    -> StmResult<T> {
    match option {
        Some(x) => Ok(x),
        None    => retry()
    }
}

#[inline]
/// Retry until a the condition holds.
///
/// # Example
///
/// ```
/// use stm::*;
/// let x = atomically(|_tx| {
///     guard(true)?; // guard(true) always succeeds.
///     Ok(42)
/// });
/// assert_eq!(x, 42);
/// ```
pub fn guard(cond: bool) -> StmResult<()> {
    if cond {
        Ok(())
    } else {
        retry()
    }
}

#[inline]
/// Optionally run a STM action. If `f` fails with a `retry()`, it does 
/// not cancel the whole transaction, but returns `None`.
///
/// # Example
///
/// ```
/// use stm::*;
/// let x:Option<i32> = atomically(|tx| 
///     optionally(tx, |_| retry()));
/// assert_eq!(x, None);
/// ```
pub fn optionally<T,F>(tx: &mut Transaction, f: F) -> StmResult<Option<T>>
    where F: Fn(&mut Transaction) -> StmResult<T>
{
    tx.or( 
        |t| f(t).map(Some),
        |_| Ok(None)
    )
}


#[cfg(test)]
mod test_lib {
    use super::*;

    #[test]
    fn infinite_retry() {
        let terminated = test::terminates(300, || { 
            let _infinite_retry: i32 = atomically(|_| retry());
        });
        assert!(!terminated);
    }

    #[test]
    fn stm_nested() {
        let var = TVar::new(0);

        let x = atomically(|tx| {
            var.write(tx, 42)?;
            var.read(tx)
        });

        assert_eq!(42, x);
    }

    /// Run multiple threads.
    ///
    /// Thread 1: Read a var, block until it is not 0 and then
    /// return that value.
    ///
    /// Thread 2: Wait a bit. Then write a value.
    ///
    /// Check if Thread 1 is woken up correctly and then check for 
    /// correctness.
    #[test]
    fn threaded() {
        use std::thread;
        use std::time::Duration;

        let var = TVar::new(0);
        // Clone for other thread.
        let varc = var.clone();

        let x = test::async(800,
            move || {
                atomically(|tx| {
                    let x = varc.read(tx)?;
                    if x == 0 {
                        retry()
                    } else {
                        Ok(x)
                    }
                })
            },
            || {
                thread::sleep(Duration::from_millis(100));

                atomically(|tx| var.write(tx, 42));
            }
        ).unwrap();

        assert_eq!(42, x);
    }


    /// test if a STM calculation is rerun when a Var changes while executing
    #[test]
    fn read_write_interfere() {
        use std::thread;
        use std::time::Duration;

        // create var
        let var = TVar::new(0);
        let varc = var.clone(); // Clone for other thread.

        // spawn a thread
        let t = thread::spawn(move || {
            atomically(|tx| {
                // read the var
                let x = varc.read(tx)?;
                // ensure that x varc changes in between
                thread::sleep(Duration::from_millis(500));

                // write back modified data this should only
                // happen when the value has not changed
                varc.write(tx, x + 10)
            });
        });

        // ensure that the thread has started and already read the var
        thread::sleep(Duration::from_millis(100));

        // now change it
        atomically(|tx| var.write(tx, 32));

        // finish and compare
        let _ = t.join();
        assert_eq!(42, var.read_atomic());
    }

    #[test]
    fn or_simple() {
        let var = TVar::new(42);

        let x = atomically(|tx| {
            tx.or(|_| {
                retry()
            },
            |tx| {
                var.read(tx)
            })
        });

        assert_eq!(x, 42);
    }

    /// A variable should not be written,
    /// when another branch was taken
    #[test]
    fn or_nocommit() {
        let var = TVar::new(42);

        let x = atomically(|tx| {
            tx.or(|tx| {
                var.write(tx, 23)?;
                retry()
            },
            |tx| {
                var.read(tx)
            })
        });

        assert_eq!(x, 42);
    }

    #[test]
    fn or_nested_first() {
        let var = TVar::new(42);

        let x = atomically(|tx| {
            tx.or(
                |tx| {
                    tx.or(
                        |_| retry(),
                        |_| retry()
                    )
                },
                |tx| var.read(tx)
            )
        });

        assert_eq!(x, 42);
    }

    #[test]
    fn or_nested_second() {
        let var = TVar::new(42);

        let x = atomically(|tx| {
            tx.or(
                |_| {
                    retry()
                },
                |t| t.or(
                    |t2| var.read(t2),
                    |_| retry()
                )
            )
        });

        assert_eq!(x, 42);
    }

    #[test]
    fn unwrap_some() {
        let x = Some(42);
        let y = atomically(|_| unwrap_or_retry(x));
        assert_eq!(y, 42);
    }

    #[test]
    fn unwrap_none() {
        let x: Option<i32> = None;
        assert_eq!(unwrap_or_retry(x), retry());
    }

    #[test]
    fn guard_true() {
        let x = guard(true);
        assert_eq!(x, Ok(()));
    }

    #[test]
    fn guard_false() {
        let x = guard(false);
        assert_eq!(x, retry());
    }

    #[test]
    fn optionally_succeed() {
        let x = atomically(|t| 
            optionally(t, |_| Ok(42)));
        assert_eq!(x, Some(42));
    }

    #[test]
    fn optionally_fail() {
        let x:Option<i32> = atomically(|t| 
            optionally(t, |_| retry()));
        assert_eq!(x, None);
    }
}
