
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

/// StmResult is a result of a single step of a STM calculation.
///
/// It informs of success or the type of failure. Normally you should not use
/// it directly. Especially recovering from an error, e.g. by using `action1.or(action2)`
/// can break the semantics of stm, and cause delayed wakeups or deadlocks.
///
/// For the later case, there is the `transaction.or(action1, action2)`, that
/// is safe to use.
pub type StmResult<T> = Result<T, StmError>;
