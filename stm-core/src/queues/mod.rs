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
    /// Check if the queue is empty.
    fn is_empty(&self, transaction: &mut Transaction) -> StmResult<bool>;
}

#[cfg(test)]
mod test {
    use std::thread;
    use std::time::Duration;

    use etest::Bencher;

    use super::TQueueLike;
    use crate::atomically;
    use crate::test;

    pub fn test_write_and_read_back<Q: 'static + TQueueLike<i32>>(mq: fn() -> Q) {
        let queue = mq();
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
    pub fn test_threaded<Q: 'static + TQueueLike<i32>>(mq: fn() -> Q) {
        let queue1 = mq();
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

    pub fn test_is_empty<Q: 'static + TQueueLike<i32>>(mq: fn() -> Q) {
        let queue = mq();
        let is_empty = atomically(|tx| queue.is_empty(tx));

        assert!(is_empty);
    }

    pub fn test_non_empty<Q: 'static + TQueueLike<i32>>(mq: fn() -> Q) {
        let queue = mq();
        atomically(|tx| queue.write(tx, 42));
        let is_empty = atomically(|tx| queue.is_empty(tx));
        assert!(!is_empty);
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
                for i in 1..n {
                    let r = atomically(|tx| queue2.read(tx));
                    assert_eq!(i, r);
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
            for i in 1..n {
                let r = atomically(|tx| chan.read(tx));
                assert_eq!(i, r);
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
                for j in 1..m {
                    let r = atomically(|tx| chan.read(tx));
                    assert_eq!(i * m + j, r);
                }
            }
        });
    }
}

/// Reuse the same test definitions for each implementation of the `TQueueLike` trait
/// by calling this macro with a function to create a new instance of the queue.
///
/// For example:
/// ```text
/// test_queue_mod!(|| { crate::queues::tchan::TChan::<i32>::new() });
/// ```
#[macro_export]
macro_rules! test_queue_mod {
    ($make:expr) => {
        #[cfg(test)]
        mod test_queue {
            use crate::queues::test as tq;

            #[test]
            fn write_and_read_back() {
                tq::test_write_and_read_back($make);
            }

            #[test]
            fn threaded() {
                tq::test_threaded($make);
            }

            #[test]
            fn is_empty() {
                tq::test_is_empty($make);
            }

            #[test]
            fn non_empty() {
                tq::test_non_empty($make);
            }
        }

        #[cfg(test)]
        mod bench_queue {
            use super::super::test as tq;
            use etest::Bencher;

            #[bench]
            fn two_threads_read_write(b: &mut Bencher) {
                tq::bench_two_threads_read_write(b, $make);
            }

            #[bench]
            fn one_thread_write_many_then_read(b: &mut Bencher) {
                tq::bench_one_thread_write_many_then_read(b, $make);
            }

            #[bench]
            fn one_thread_repeat_write_read(b: &mut Bencher) {
                tq::bench_one_thread_repeat_write_read(b, $make);
            }
        }
    };
}
