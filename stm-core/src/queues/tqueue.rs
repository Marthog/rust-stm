use super::TQueueLike;
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
}

#[cfg(test)]
mod test {
    use super::super::test as tq;
    use super::TQueue;
    use etest::Bencher;

    #[test]
    fn write_and_read_back() {
        tq::write_and_read_back(TQueue::<i32>::new());
    }

    #[test]
    fn threaded() {
        tq::threaded(TQueue::<i32>::new());
    }

    #[bench]
    fn bench_two_threads_read_write(b: &mut Bencher) {
        tq::bench_two_threads_read_write(b, || TQueue::<i32>::new());
    }

    #[bench]
    fn bench_one_thread_write_many_then_read(b: &mut Bencher) {
        tq::bench_one_thread_write_many_then_read(b, || TQueue::<i32>::new());
    }

    #[bench]
    fn bench_one_thread_repeat_write_read(b: &mut Bencher) {
        tq::bench_one_thread_repeat_write_read(b, || TQueue::<i32>::new());
    }
}
