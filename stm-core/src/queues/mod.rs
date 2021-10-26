mod tbqueue;
mod tchan;
mod tqueue;
mod tvecdequeue;

use crate::{StmResult, Transaction};

/// Transactional queue-like structure.
///
/// This is a common interface between the various implementations in Simon Marlow's book.
pub trait TQueueLike<T>: Clone + Send {
    /// Pop the head of the queue, or retry until there is an element if it's empty.
    fn read(&self, transaction: &mut Transaction) -> StmResult<T>;
    /// Push to the end of the queue.
    fn write(&self, transaction: &mut Transaction, value: T) -> StmResult<()>;
}

#[cfg(test)]
mod test {
    use std::thread;
    use std::time::Duration;

    use etest::Bencher;

    use super::TQueueLike;
    use crate::atomically;
    use crate::test;

    pub fn write_and_read_back<Q: TQueueLike<i32>>(queue: Q) {
        let (x, y) = atomically(|tx| {
            queue.write(tx, 42)?;
            queue.write(tx, 31)?;
            let x = queue.read(tx)?;
            let y = queue.read(tx)?;
            Ok((x, y))
        });

        assert_eq!(42, x);
        assert_eq!(31, y);
    }

    /// Run multiple threads.
    ///
    /// Thread 1: Read from the channel, block until it's non-empty, then return the value.
    ///
    /// Thread 2: Wait a bit, then write a value.
    ///
    /// Check that Thread 1 has been woken up to read the value written by Thread 2.
    pub fn threaded<Q: 'static + TQueueLike<i32>>(queue: Q) {
        let queue1 = queue;
        // Clone for Thread 2
        let queue2 = queue1.clone();

        let x = test::run_async(
            500,
            move || atomically(|tx| queue2.read(tx)),
            || {
                thread::sleep(Duration::from_millis(100));
                atomically(|tx| queue1.write(tx, 42))
            },
        )
        .unwrap();

        assert_eq!(42, x);
    }

    // Benchmarks based on https://github.com/simonmar/parconc-examples/blob/master/chanbench.hs

    /// Two threads, one reading from and one writing to the channel.
    pub fn bench_two_threads_read_write<Q: 'static + TQueueLike<i32>>(
        b: &mut Bencher,
        mq: fn() -> Q,
    ) {
        b.iter(|| {
            let n = 1000;
            let queue1 = mq();
            let queue2 = queue1.clone();

            let t1 = thread::spawn(move || {
                for i in 1..n {
                    atomically(|tx| queue1.write(tx, i));
                }
            });

            let t2 = thread::spawn(move || {
                for _ in 1..n {
                    let _ = atomically(|tx| queue2.read(tx));
                }
            });

            t1.join().unwrap();
            t2.join().unwrap();
        });
    }

    /// One thread, writing a large number of values then reading them.
    pub fn bench_one_thread_write_many_then_read<Q: TQueueLike<i32>>(
        b: &mut Bencher,
        mq: fn() -> Q,
    ) {
        b.iter(|| {
            let n = 1000;
            let chan = mq();

            for i in 1..n {
                atomically(|tx| chan.write(tx, i));
            }
            for _ in 1..n {
                atomically(|tx| chan.read(tx));
            }
        });
    }

    // One thread, repeatedly writing and then reading a number of values.
    pub fn bench_one_thread_repeat_write_read<Q: TQueueLike<i32>>(b: &mut Bencher, mq: fn() -> Q) {
        b.iter(|| {
            let n = 1000;
            let m = 100;
            let chan = mq();

            for i in 1..(n / m) {
                for j in 1..m {
                    atomically(|tx| chan.write(tx, i * m + j));
                }
                for _ in 1..m {
                    atomically(|tx| chan.read(tx));
                }
            }
        });
    }
}
