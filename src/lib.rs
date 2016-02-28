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
//! With locks the sequence
//! of two threadsafe actions is no longer threadsafe because
//! other threads may interfer in between of these actions.
//! Applying another lock may lead to common sources of errors
//! like deadlocks and forgotten locks.
//!
//! Unlike locks Software transactional memory is composable.
//! It is typically implemented by writing all read and write
//! operations in a log. When the action has finished, the reads
//! are checked for consistency and depending on the result
//! either the writes are committed in a single atomic operation
//! or the result is discarded and the computation run again.
//!
//! # Usage
//!
//! You should only use the functions that are safe to use.
//!
//! Don't have side effects except for the atomic variables, from this library.
//! Especially a mutex or other blocking mechanisms inside of software transactional
//! memory is dangerous.
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
//! and call `try!` on the result. You should not handle the error yourself.
//!
//! ```
//! use stm::{atomically, TVar};
//! let var = TVar::new(0);
//!
//! let x = atomically(|trans| {
//!     try!(var.write(trans, 42));
//!     var.read(trans) // return the value saved in var
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
//! because stm is designed to be run multiple times. Return a
//! closure if you have to.
//! * Don't handle the error types yourself, unless you absolutely know, what you
//! are doing. Use `Transaction::or`, to try alternatives.
//! * Don't run `atomically` inside of another, because your thread will
//! immediately panic. When you use STM in the inner of a function then
//! express that in the public interface, by taking a `&mut Transaction` and 
//! returning a `StmResult<T>`. Callers can safely compose it into
//! larger blocks.
//! * Don't mix locks and transactions. Your code will easily deadlock and slow
//! down on unpredictably.
//! * When you put an `Arc` into a `Var`, don't use inner mutability
//! to modify it because, the inner still points to the original value.
//!
//! # Speed
//!
//! Generally keep your atomic blocks as small as possible, because
//! the more time you spend, the more likely it is, to collide with
//! other threads. For STM, reading vars is quite slow, because it
//! needs to look them up in the log every time, they are written to,
//! and every used var increases the chance of collisions. You should
//! keep the amount of accessed variables as low as needed.

mod transaction;
mod var;
mod result;

#[cfg(test)]
mod test;

pub use var::TVar;
pub use transaction::Transaction;
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

#[test]
fn test_infinite_retry() {
    let terminated = test::terminates(50, || { 
        let _infinite_retry: i32 = atomically(|_| retry());
    });
    assert!(!terminated);
}

#[test]
fn test_stm_nested() {
    let var = TVar::new(0);

    let x = atomically(|trans| {
        try!(var.write(trans, 42));
        var.read(trans)
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
fn test_threaded() {
    use std::thread;
    use std::time::Duration;

    let var = TVar::new(0);
    let var_ref = var.clone();

    let x = test::async(200,
        move || {
            atomically(|trans| {
                let x = try!(var_ref.read(trans));
                if x == 0 {
                    retry()
                } else {
                    Ok(x)
                }
            })
        },
        move || {
            thread::sleep(Duration::from_millis(100));

            atomically(|trans| var.write(trans, 42));
        }
    ).unwrap();

    assert_eq!(42, x);
}

/// test if a STM calculation is rerun when a Var changes while executing
#[test]
fn test_read_write_interfere() {
    use std::thread;
    use std::time::Duration;

    // create var
    let var = TVar::new(0);
    let var_ref = var.clone();

    // spawn a thread
    let t = thread::spawn(move || {
        atomically(|log| {
            // read the var
            let x = try!(var_ref.read(log));
            // ensure that x var_ref changes in between
            thread::sleep(Duration::from_millis(500));

            // write back modified data this should only
            // happen when the value has not changed
            var_ref.write(log, x + 10)
        });
    });

    // ensure that the thread has started and already read the var
    thread::sleep(Duration::from_millis(100));

    // now change it
    atomically(|trans| var.write(trans, 32));

    // finish and compare
    let _ = t.join();
    assert_eq!(42, var.read_atomic());
}

#[test]
fn test_or_simple() {
    let var = TVar::new(42);

    let x = atomically(|trans| {
        trans.or(|_| {
            retry()
        },
        |trans| {
            var.read(trans)
        })
    });

    assert_eq!(x, 42);
}

/// A variable should not be written,
/// when another branch was taken
#[test]
fn test_or_nocommit() {
    let var = TVar::new(42);

    let x = atomically(|trans| {
        trans.or(|trans| {
            try!(var.write(trans, 23));
            retry()
        },
        |trans| {
            var.read(trans)
        })
    });

    assert_eq!(x, 42);
}

#[test]
fn test_or_nested_first() {
    let var = TVar::new(42);

    let x = atomically(|trans| {
        trans.or(
            |t| {
                t.or(
                    |_| retry(),
                    |_| retry()
                )
            },
            |trans| var.read(trans)
        )
    });

    assert_eq!(x, 42);
}

#[test]
fn test_or_nested_second() {
    let var = TVar::new(42);

    let x = atomically(|trans| {
        trans.or(
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
