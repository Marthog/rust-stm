// Copyright 2015-2016 rust-stm Developers
//
// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


/// call a STM function from inside of a STM block
/// 
/// if we had control over stack unwinding we could use that
#[macro_export]
macro_rules! stm_call {
    ( $e:expr )     => ({
        use $crate::StmResult::*;

        let ret = $e.intern_run();
        match ret {
            Success(s)  => s,
            Retry       => return Retry,
            Failure     => return Failure
        }
    })
}


/// declare a block that uses STM
#[macro_export]
macro_rules! stm {
    ( $e:expr )    => {{
        let func = || {
            $crate::StmResult::Success($e)
        };
        $crate::STM::new(func)
    }}
}
