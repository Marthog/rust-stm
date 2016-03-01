# Software Transactional Memory [![Build Status](https://travis-ci.org/Marthog/rust-stm.svg?branch=master)](https://travis-ci.org/Marthog/rust-stm)


This library implements [software transactional memory]
(https://en.wikipedia.org/wiki/Software_transactional_memory),
often abbreviated with STM.

It is designed closely to haskells STM library. Read Simon Marlow's
[Parallel and Concurrent Programming in Haskell]
(http://chimera.labs.oreilly.com/books/1230000000929/ch10.html)
for more info. Especially the chapter about [Performance]
(http://chimera.labs.oreilly.com/books/1230000000929/ch10.html#sec_stm-cost)
is also important for using STM in rust.

With locks the sequence
of two threadsafe actions is no longer threadsafe because
other threads may interfer in between of these actions.
Applying another lock may lead to common sources of errors
like deadlocks and forgotten locks.

Unlike locks Software transactional memory is composable.
It is typically implemented by writing all read and write
operations in a log. When the action has finished, the reads
are checked for consistency and depending on the result
either the writes are committed in a single atomic operation
or the result is discarded and the computation run again.


# Usage

 You should only use the functions that are safe to use.

 Don't have side effects except for the atomic variables, from this library.
 Especially a mutex or other blocking mechanisms inside of software transactional
 memory is dangerous.

 You can run the top-level atomic operation by calling `atomically`.


 ```rust
 use stm::atomically;
 atomically(|trans| {
     // some action
     // return value as `Result`, for example
     Ok(42)
 });
 ```

 Calls to `atomically` should not be nested.

 For running an atomic operation inside of another, pass a mutable reference to a `Transaction`
 and call `try!` on the result. You should not handle the error yourself.

 ```rust
 use stm::{atomically, TVar};
 let var = TVar::new(0);

 let x = atomically(|trans| {
     try!(var.write(trans, 42));
     var.read(trans) // return the value saved in var
 });

 println!("var = {}", x);

 ```

## License

Licensed under either of

 * Apache License, Version 2.0, ([LICENSE-APACHE](LICENSE-APACHE) or http://www.apache.org/licenses/LICENSE-2.0)
 * MIT license ([LICENSE-MIT](LICENSE-MIT) or http://opensource.org/licenses/MIT)

at your option.

### Contribution

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.
