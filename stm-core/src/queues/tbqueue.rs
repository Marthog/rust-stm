use super::TQueueLike;
use crate::test_queue_mod;
use crate::{guard, retry, StmResult, TVar, Transaction};
use std::any::Any;

/// Bounded queue using two vectors.
///
/// Similar to `TQueue` but every read and write touches a common `TVar`
/// to track the current capacity, retrying if the queue is full.
#[derive(Clone)]
pub struct TBQueue<T> {
    capacity: TVar<u32>,
    read: TVar<Vec<T>>,
    write: TVar<Vec<T>>,
}

impl<T> TBQueue<T>
where
    T: Any + Sync + Send + Clone,
{
    /// Create an empty `TBQueue`.
    #[allow(dead_code)]
    pub fn new(capacity: u32) -> TBQueue<T> {
        TBQueue {
            capacity: TVar::new(capacity),
            read: TVar::new(Vec::new()),
            write: TVar::new(Vec::new()),
        }
    }
}

impl<T> TQueueLike<T> for TBQueue<T>
where
    T: Any + Sync + Send + Clone,
{
    fn write(&self, transaction: &mut Transaction, value: T) -> StmResult<()> {
        let capacity = self.capacity.read(transaction)?;
        guard(capacity > 0)?;
        self.capacity.write(transaction, capacity - 1)?;

        // Same as TQueue.
        let mut v = self.write.read(transaction)?;
        v.push(value);
        self.write.write(transaction, v)
    }

    fn read(&self, transaction: &mut Transaction) -> StmResult<T> {
        let capacity = self.capacity.read(transaction)?;
        self.capacity.write(transaction, capacity + 1)?;

        // Same as TQueue.
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

test_queue_mod!(|| { crate::queues::tbqueue::TBQueue::<i32>::new(1_000_000) });

#[cfg(test)]
mod test {
    use super::{TBQueue, TQueueLike};
    use crate::test;
    use crate::{atomically, optionally};
    use std::thread;
    use std::time::Duration;

    #[test]
    fn threaded_bounded_blocks() {
        let queue = TBQueue::<i32>::new(1);

        let terminated = test::terminates(300, move || {
            atomically(|tx| {
                queue.write(tx, 1)?;
                queue.write(tx, 2)
            });
        });

        assert!(!terminated);
    }

    #[test]
    fn threaded_bounded_unblocks() {
        let queue1 = TBQueue::<i32>::new(1);
        let queue2 = queue1.clone();

        let terminated = test::terminates_async(
            500,
            move || {
                // Don't try to write 2 items at the same time or both will be retried,
                // and the reader will retry because of an empty queue.
                atomically(|tx| queue2.write(tx, 1));
                atomically(|tx| queue2.write(tx, 2));
            },
            || {
                thread::sleep(Duration::from_millis(100));
                atomically(|tx| optionally(tx, |tx| queue1.read(tx)));
            },
        );

        assert!(terminated);
    }
}
