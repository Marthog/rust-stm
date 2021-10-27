use super::TQueueLike;
use crate::test_queue_mod;
use crate::{retry, StmResult, TVar, Transaction};
use std::any::Any;

/// Unbounded queue using two vectors.
///
/// This implementation writes to one vector and reads from the other
/// until the reads vector becomes empty and the two need to be swapped.
/// Again reads don't block writes most of the time. It has an amortised
/// cost of O(1).
#[derive(Clone)]
pub struct TQueue<T> {
    read: TVar<Vec<T>>,
    write: TVar<Vec<T>>,
}

impl<T> TQueue<T>
where
    T: Any + Sync + Send + Clone,
{
    /// Create an empty `TQueue`.
    #[allow(dead_code)]
    pub fn new() -> TQueue<T> {
        TQueue {
            read: TVar::new(Vec::new()),
            write: TVar::new(Vec::new()),
        }
    }
}

impl<T> TQueueLike<T> for TQueue<T>
where
    T: Any + Sync + Send + Clone,
{
    fn write(&self, transaction: &mut Transaction, value: T) -> StmResult<()> {
        let mut v = self.write.read(transaction)?;
        v.push(value);
        self.write.write(transaction, v)
    }

    fn read(&self, transaction: &mut Transaction) -> StmResult<T> {
        let mut rv = self.read.read(transaction)?;
        // Elements are stored in reverse order.
        match rv.pop() {
            Some(value) => {
                self.read.write(transaction, rv)?;
                Ok(value)
            }
            None => {
                let mut wv = self.write.read(transaction)?;
                if wv.is_empty() {
                    retry()
                } else {
                    wv.reverse();
                    let value = wv.pop().unwrap();
                    self.read.write(transaction, wv)?;
                    self.write.write(transaction, Vec::new())?;
                    Ok(value)
                }
            }
        }
    }

    fn is_empty(&self, transaction: &mut Transaction) -> StmResult<bool> {
        if self.read.read(transaction)?.is_empty() {
            Ok(self.write.read(transaction)?.is_empty())
        } else {
            Ok(false)
        }
    }
}

test_queue_mod!(|| { crate::queues::tqueue::TQueue::<i32>::new() });
