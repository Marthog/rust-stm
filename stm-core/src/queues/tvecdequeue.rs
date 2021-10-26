use super::TQueueLike;
use crate::{retry, StmResult, TVar, Transaction};
use std::{any::Any, collections::VecDeque};

#[derive(Clone)]
/// Unbounded queue backed by a single `VecDequeue`.
///
/// The drawback is that reads and writes both touch the same `TVar`.
pub struct TVecDequeue<T> {
    queue: TVar<VecDeque<T>>,
}

impl<T> TVecDequeue<T>
where
    T: Any + Sync + Send + Clone,
{
    /// Create an empty `TVecDequeue`.
    #[allow(dead_code)]
    pub fn new() -> TVecDequeue<T> {
        TVecDequeue {
            queue: TVar::new(VecDeque::new()),
        }
    }
}

impl<T> TQueueLike<T> for TVecDequeue<T>
where
    T: Any + Sync + Send + Clone,
{
    fn write(&self, transaction: &mut Transaction, value: T) -> StmResult<()> {
        let mut queue = self.queue.read(transaction)?;
        queue.push_back(value);
        self.queue.write(transaction, queue)
    }

    fn read(&self, transaction: &mut Transaction) -> StmResult<T> {
        let mut queue = self.queue.read(transaction)?;
        match queue.pop_front() {
            None => retry(),
            Some(value) => {
                self.queue.write(transaction, queue)?;
                Ok(value)
            }
        }
    }
}

#[cfg(test)]
mod test {
    use super::super::test as tq;
    use super::TVecDequeue;
    use etest::Bencher;

    #[test]
    fn write_and_read_back() {
        tq::write_and_read_back(TVecDequeue::<i32>::new());
    }

    #[test]
    fn threaded() {
        tq::threaded(TVecDequeue::<i32>::new());
    }

    #[bench]
    fn bench_two_threads_read_write(b: &mut Bencher) {
        tq::bench_two_threads_read_write(b, || TVecDequeue::<i32>::new());
    }

    #[bench]
    fn bench_one_thread_write_many_then_read(b: &mut Bencher) {
        tq::bench_one_thread_write_many_then_read(b, || TVecDequeue::<i32>::new());
    }

    #[bench]
    fn bench_one_thread_repeat_write_read(b: &mut Bencher) {
        tq::bench_one_thread_repeat_write_read(b, || TVecDequeue::<i32>::new());
    }
}
