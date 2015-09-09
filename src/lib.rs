

// need to be replaced by a proper semaphore implementation
// or other alternative so that is compiles on stable
#![feature(semaphore)]
#![allow(dead_code)]
#![allow(unused_imports)]

mod stm;
mod log;
mod var;

pub use stm::{STM, StmResult, retry};

pub use var::{Var};

