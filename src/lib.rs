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
//! STM operations are safed inside the type `STM<T>` where T
//! is the return type of the inner operation. They are created with `stm!`:
//!
//! ```
//! # #[macro_use] extern crate stm;
//! # fn main() {
//! let transaction = stm!(trans => {
//!     // some action
//!     // return value (or empty for unit)
//! });
//! # }
//! ```
//! and can then be run by calling.
//!
//! ```
//! # #[macro_use] extern crate stm;
//! # fn main() {
//! # let transaction = stm!(trans => {
//! # });
//! transaction.atomically();
//! # }
//!
//! ```
//!
//! For running an STM-Block inside of another
//! use the macro `stm_call!`:
//!
//! ```
//! # #[macro_use] extern crate stm;
//! # fn main() {
//! use stm::Var;
//! let var = Var::new(0);
//! let modify = stm!(trans => {
//!     var.write(trans, 42);
//! });
//!
//! let x = stm!(trans => {
//!     stm_call!(trans, modify);
//!     var.read(trans) // return the value saved in var
//! }).atomically();
//!
//! println!("var = {}", x);
//! # }
//!
//! ```
//!
//! # STM safety
//!
//! Software transactional memory is completely safe in the terms
//! that rust considers safe. Still there are multiple rules that
//! you should obey when dealing with software transactional memory:
//!
//! * Don't run code with side effects, especially no IO-code,
//! because stm is designed to be run multiple times. Return a
//! closure if you have to.
//! * Don't run an STM-Block by calling `STM::atomically` inside of
//! another because your thread will
//! immediately panic. When you use STM in the inner of a function then
//! return a STM-Object instead so that callers can safely compose it into
//! larger blocks.
//! * Don't mix locks and STM. Your code will easily deadlock or slow
//! down unpredictably.
//! * When you put an `Arc` into a `Var` don't use inner mutability
//! to modify it since the inner still points to the original value.
//!
//!
//! # Speed
//! Generally keep your atomic blocks as small as possible, because
//! the more time you spend, the more likely it is to collide with
//! other threads. For STMm reading vars is quite slow because it
//! needs to look them up in the log every time they are written to
//! and every used var increases the chance of collisions. You should
//! keep the amount of accessed variables as low as needed.

#[macro_use]
mod macros;

mod stm;
mod transaction;
mod var;

pub use stm::{STM, StmResult, retry};
pub use var::Var;
pub use transaction::Transaction;


#[cfg(test)]
use std::thread;

#[cfg(test)]
use std::sync::{Arc};

#[cfg(test)]
use std::time::Duration;

/*
#[test]
fn test_stm_macro() {
    let var = Var::new(0);

    let stm = stm!(stm => {
        stm.write(&var, 42);
        0
    });

    stm.atomically();
}

#[test]
fn test_stm_nested() {
    let var = Var::new(0);

    let inner_stm = stm!(stm => stm.write(&var, 42));
    let calc = stm!(stm => {
        stm_call!(stm, inner_stm);
        stm.read(&var)
    });

    assert_eq!(42, calc.atomically());
}

#[test]
fn test_threaded() {
    use std::time::Duration;
    use std::sync::mpsc::channel;

    let var = Arc::new(Var::new(0));

    let (tx, rx) = channel();

    let var_ref = var.clone();
    thread::spawn(move || {
        let stm = stm!(log => {
            let x = log.read(&var_ref);
            if x == 0 {
                stm_try!(retry());
            }
            x
        });

        let _ = tx.send(stm.atomically());
    });

    thread::sleep(Duration::from_millis(100));

    stm!(log => {
        log.write(&var, 42);
    })
        .atomically();

    let x = rx.recv().unwrap();

    assert_eq!(42, x);
}

/// test if a STM calculation is rerun when a Var changes while executing
#[test]
fn test_read_write_interfere() {

    // create var
    let var = Arc::new(Var::new(0));
    let var_ref = var.clone();

    // spawn a thread
    let t = thread::spawn(move || {
        stm!(log => {
            // read the var
            let x = var_ref.read(log);
            // ensure that x var_ref changes in between
            thread::sleep(Duration::from_millis(200));

            // write back modified data this should only
            // happen when the value has not changed
            var_ref.write(log, x + 10);
        })
            .atomically();
    });

    // ensure that the thread has started and already read the var
    thread::sleep(Duration::from_millis(10));

    // now change it
    stm!(log => {
        var.write(log, 32);
    })
        .atomically();

    // finish and compare
    let _ = t.join();
    assert_eq!(42, var.read_atomic());
}
*/

// #[bench]
// fn bench_counter_stm(bench: &mut Bencher) {
// let var = Var::new(0);
//
// let stm = stm!({
// let x = var.read();
// var.write(x+1);
// });
//
// b.iter(|| stm.atomically());
// }
//
// #[bench]
// fn bench_counter_lock(bench: &mut Bencher) {
// let var = Mutex::new(0);
//
// b.iter(|| {
// let guard = var.lock().unwrap();
// guard += 1;
// });
// }
//
