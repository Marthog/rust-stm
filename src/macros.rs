
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
        let func = move || {
            $crate::StmResult::Success($e)
        };
        $crate::STM::new(func)
    }}
}

