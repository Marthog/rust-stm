# Software Transactional Memory [![Build Status](https://travis-ci.org/Marthog/rust-stm.svg?branch=master)](https://travis-ci.org/Marthog/rust-stm)


This library implements [software transactional memory](https://en.wikipedia.org/wiki/Software_transactional_memory),
often abbreviated with STM.

It is designed closely to haskells STM library. Read Simon Marlow's
[Parallel and Concurrent Programming in Haskell](http://chimera.labs.oreilly.com/books/1230000000929/ch10.html)
for more info. Especially the chapter about [Performance](http://chimera.labs.oreilly.com/books/1230000000929/ch10.html#sec_stm-cost)
is also important for using STM in rust.

With locks the sequential composition of two 
two threadsafe actions is no longer threadsafe because
other threads may interfer in between of these actions.
Applying a third lock to protect both may lead to common sources of errors
like deadlocks or race conditions.

Unlike locks Software transactional memory is composable.
It is typically implemented by writing all read and write
operations in a log. When the action has finished and
all the used `TVar`s are consistend, the writes are commited as
a single atomic operation.
Otherwise the computation repeats. This may lead to starvation,
but avoids common sources of bugs.

Panicing within STM does not poison the `TVar`s. STM ensures consistency by
never committing on panic.

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
and call `try!` on the result or use `?`. You should not handle the error yourself, because it
breaks consistency.

```rust
use stm::{atomically, TVar};
let var = TVar::new(0);

let x = atomically(|trans| {
    var.write(trans, 42)?; // Pass failure to parent.
    var.read(trans) // Return the value saved in var.
});

println!("var = {}", x);

```

# STM safety

Software transactional memory is completely safe in the terms,
that rust considers safe. Still there are multiple rules that
you should obey when dealing with software transactional memory:

* Don't run code with side effects, especially no IO-code,
because stm repeats the computation when it detects inconsistent state.
Return a closure if you have to.
* Don't handle the error types yourself, unless you absolutely know, what you
are doing. Use `Transaction::or`, to combine alternative paths. Always call `try!` or
`?` and never ignore a `StmResult`.
* Don't run `atomically` inside of another. `atomically` is designed to have side effects
and will therefore break stm's assumptions. Nested calls are detected at runtime and
handled with panic.
When you use STM in the inner of a function, then
express it in the public interface, by taking `&mut Transaction` as parameter and 
returning `StmResult<T>`. Callers can safely compose it into
larger blocks.
* Don't mix locks and transactions. Your code will easily deadlock or slow
down on unpredictably.
* Don't use inner mutability to change the content of a `TVar`.

# Speed

Generally keep your atomic blocks as small as possible, because
the more time you spend, the more likely it is, to collide with
other threads. For STM, reading `TVar`s is quite slow, because it
needs to look them up in the log every time.
Every used `TVar` increases the chance of collisions. Therefore you should
keep the amount of accessed variables as low as needed.

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
