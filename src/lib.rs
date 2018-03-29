// Copyright 2015-2018 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.

//! This library implements
//! [software transactional memory](https://en.wikipedia.org/wiki/Software_transactional_memory),
//! often abbreviated with STM.
//!
//! It is designed closely to haskells STM library. Read Simon Marlow's
//! *Parallel and Concurrent Programming in Haskell*
//! for more info. Especially the chapter about
//! Performance is also important for using STM in rust.
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
//! all the used `TVar`s are consistent, the writes are commited as
//! a single atomic operation.
//! Otherwise the computation repeats. This may lead to starvation,
//! but avoids common sources of bugs.
//!
//! Panicing within STM does not poison the `TVar`s. STM ensures consistency by
//! never committing on panic.
//!
//! # Usage
//!
//! You should only use the functions that are transaction-safe.
//! Transaction-safe functions don't have side effects, except those provided by `TVar`.
//! Mutexes and other blocking mechanisms are especially dangerous, because they can
//! interfere with the internal locking scheme of the transaction and therefore
//! cause deadlocks.
//! 
//! Note, that Transaction-safety does *not* mean safety in the rust sense, but is a
//! subset of allowed behavior. Even if code is not transaction-safe, no segmentation
//! faults will happen.
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
//! Nested calls to `atomically` are not allowed. A run-time check prevents this.
//! Instead of using atomically internally, add a `&mut Transaction` parameter and
//! return `StmResult`.
//!
//! Use ? on `StmResult`, to propagate a transaction error through the system.
//! Do not handle the error yourself.
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
//! // var = 42
//!
//! ```
//!
//! # Transaction safety
//!
//! Software transactional memory is completely safe in the rust sense, so
//! undefined behavior will never occur.
//! Still there are multiple rules that
//! you should obey when dealing with software transactional memory.
//!
//! * Don't run code with side effects, especially no IO-code.
//! Transactions repeat in failure cases. Using IO would repeat this IO-code.
//! Return a closure if you have to.
//! * Don't handle `StmResult` yourself.
//! Use `Transaction::or` to combine alternative paths and `optionally` to check if an inner
//! function has failed. Always use `?` and 
//! never ignore a `StmResult`.
//! * Don't run `atomically` inside of another. `atomically` is designed to have side effects
//! and will therefore break transaction safety. 
//! Nested calls are detected at runtime and handled with panicking.
//! When you use STM in the inner of a function, then
//! express it in the public interface, by taking `&mut Transaction` as parameter and 
//! returning `StmResult<T>`. Callers can safely compose it into
//! larger blocks.
//! * Don't mix locks and transactions. Your code will easily deadlock or slow
//! down unpredictably.
//! * Don't use inner mutability to change the content of a `TVar`.
//!
//! Panicking in a transaction is transaction-safe. The transaction aborts and 
//! all changes are discarded. No poisoning or half written transactions happen.
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

extern crate stm_core;

// Reexport everything from stm-core.
pub use stm_core::*;
