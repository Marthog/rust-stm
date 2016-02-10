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
    ( $log:expr , $e:expr )     => ({
        use $crate::StmResult::*;

        let ret = $e.intern_run($log);
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
    ( $i:ident => $e:expr )    => {{
        $crate::STM::new(|$i| {
            $crate::StmResult::Success($e)
        })
    }}
}
