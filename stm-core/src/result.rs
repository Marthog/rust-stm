use std::ops::Try;

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub struct StmResult<T>{
    inner: Result<StmError, T>
}

impl<T> StmResult<T> {
    pub fn new(value: T) -> Self {
        StmResult{ inner: Ok(value) }
    }
}

#[derive(Eq, PartialEq, Clone, Copy, Debug)]
pub enum StmError {
    /// The call failed, because a variable, the computation
    /// depends on, has changed.
    Failure,

    /// `retry` was called.
    ///
    /// It may block until at least one read variable has changed.
    Retry,
}


impl<T> Try for StmResult<T> {
    type Ok = T;
    type Error = StmError;

    fn into_result(self) -> Self {
        self.inner
    }
    
    fn from_ok(v: T) -> Self {
        StmResult::new(v)
    }

    fn from_error(v: StmError) -> Self {
        StmResult{inner: Result::from_error(v)}
    }
}
