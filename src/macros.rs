
/// call a STM function from inside of a STM block
/// 
/// if we had control over stack unwinding we could use that
macro_rules! stm_call {
    ( $e:expr )     => ({
        use $crate::stm::StmResult::*;

        let ret = unsafe { $e.intern_run() };
        match ret {
            Success(s)  => s,
            Retry       => return Retry,
            Failure     => return Failure
        }
    })
}


/// declare a block that uses STM
macro_rules! stm {
    ( $e:expr )    => {{
        use $crate::stm::StmResult;

        let func = move || {
            StmResult::Success($e)
        };
        STM::new(func)
    }}
}

