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
//! let transaction = stm!({
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
//! # let transaction = stm!({
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
//! let modify = stm!({
//!     var.write(42);
//! });
//!
//! let x = stm!({
//!     stm_call!(modify);
//!     var.read()  // return the value saved in var
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
//! * Don't mix locks and STM. Your code will easily deadlock and slow
//! down on unpredictably.
//! * When you put an `Arc` into a `Var` don't use inner mutability
//! to modify it since the inner still points to the original value.
//! * Don't call `Var::read` or `Var::write` from outside of a STM block.
//! Instead start a STM or use `Var::read_atomic` for it.
//! 
//!
//! # Speed
//! Generally keep your atomic blocks as small as possible bacause
//! the more time you spend the more likely it is to collide with
//! other threads. For STM reading vars is quite slow because it
//! need to look them up in the log every time they are written to
//! and every used var increases the chance of collisions. You should
//! keep the amount of accessed variables as low as needed.

#[macro_use]
mod macros;

mod stm;
mod log;
mod var;

pub use stm::{STM, StmResult, retry};

pub use var::Var;


#[test]
fn test_stm_macro() {
    let var = Var::new(0);

    let stm = stm!({
        var.write(42);
        0
    });

    stm.atomically();
}



#[test]
fn test_stm_nested() {
    let var = Var::new(0);

    let inner_stm = stm!(var.write(42));
    let stm = stm!({
        stm_call!(inner_stm);
        var.read()
    });

    assert_eq!(42, stm.atomically());
}


#[test]
fn test_threaded() {
    use std::time::Duration;
    use std::thread;
    use std::sync::mpsc::channel;

    let var = Var::new(0);

    let (tx, rx) = channel();

    let var_ref = var.clone();
    thread::spawn(move || {
        let stm = stm!({
            let x = var_ref.read();
            if x==0 {
                stm_call!(retry());
            }
            x
        });

        let _ = tx.send(stm.atomically());
    });

    thread::sleep(Duration::from_millis(100));

    stm!({
        var.write(42);
    }).atomically();

    let x = rx.recv().unwrap();

    assert_eq!(42, x);
}

/// test if a STM calculation is rerun when a Var changes while executing
#[test]
fn test_read_write_interfere() {
    use std::thread;
    use std::time::Duration;

    // create var
    let var = Var::new(0);
    let var_ref = var.clone();

    // spawn a thread
    let t = thread::spawn(move || {
        stm!({
            // read the var
            let x = var_ref.read();
            // ensure that x var_ref changes in between
            thread::sleep(Duration::from_millis(200));

            // write back modified data this should only
            // happen when the value has not changed
            var_ref.write(x+10);
        }).atomically();
    });

    // ensure that the thread has started and already read the var
    thread::sleep(Duration::from_millis(10));

    // now change it
    stm!({
        var.write(32);
    }).atomically();

    // finish and compare
    let _ = t.join();
    assert_eq!(42, var.read_atomic());
}

/*
#[bench]
fn bench_counter_stm(bench: &mut Bencher) {
    let var = Var::new(0);

    let stm = stm!({
        let x = var.read();
        var.write(x+1);
    });

    b.iter(|| stm.atomically());
}

#[bench]
fn bench_counter_lock(bench: &mut Bencher) {
    let var = Mutex::new(0); 

    b.iter(|| {
        let guard = var.lock().unwrap();
        *guard += 1;
    });
}
*/
