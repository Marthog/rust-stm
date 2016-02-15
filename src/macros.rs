// Licensed under the Apache License, Version 2.0 <LICENSE-APACHE or
// http://www.apache.org/licenses/LICENSE-2.0> or the MIT license
// <LICENSE-MIT or http://opensource.org/licenses/MIT>, at your
// option. This file may not be copied, modified, or distributed
// except according to those terms.


/// Call a STM function from inside of a STM block.
#[macro_export]
macro_rules! stm_call {
    ( $log:expr , $e:expr )     => (
        stm_try!($e.run($log))
    )
}

#[macro_export]
macro_rules! stm_try {
    ( $e:expr )     => ({
        use $crate::StmResult::*;

        match $e {
            Success(s)  => s,
            Retry       => return Retry,
            Failure     => return Failure
        }
    })
}

/// declare a block that uses STM
#[macro_export]
macro_rules! stm {
    ( $i:ident => $e:expr )    => {{
        $crate::STM::new(|$i| {
            $crate::StmResult::Success($e)
        })
    }}
}
