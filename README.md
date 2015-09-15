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

STM operations are safed inside the type `STM<T>` where T
is the return type of the inner operation. They are created with `stm!`:

```
let transaction = stm!({
    // some action
    // return value (or empty for unit)
});
```
and can then be run by calling.

```
transaction.atomically();

```

For running an STM-Block inside of another
use the macro `stm_call!`:

```
use stm::Var;
let var = Var::new(0);
let var2 = var.clone(); // clone so that it can be send to a closure
let modify = stm!({
    var2.write(42);
});

let x = stm!({
    stm_call!(modify);
    var.read()  // return the value saved in var
}).atomically();

println!("var = {}", x);

```
