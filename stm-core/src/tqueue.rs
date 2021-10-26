use std::any::Any;

use crate::{retry, StmResult, TVar, Transaction};

/// Transactional queue-like structure.
///
/// This is a common interface between the various implementations in Simon Marlow's book.
pub trait TQueueLike<T>: Clone + Send {
    /// Pop the head of the queue, or retry until there is an element if it's empty.
    fn read(&self, transaction: &mut Transaction) -> StmResult<T>;
    /// Push to the end of the queue.
    fn write(&self, transaction: &mut Transaction, value: T) -> StmResult<()>;
}

/// A `TVar` that can be empty, or be a cons cell of an item and
/// the tail of the list, which is also a `TVarList`.
type TVarList<T> = TVar<TList<T>>;

/// A linked list of `TVar`s.
#[derive(Clone)]
enum TList<T> {
    TNil,
    TCons(T, TVarList<T>),
}

/// Implementation of an ubounded queue based on Simon Marlow's book.
///
/// This implementation builds up a linked list of `TVar`s with a
/// read and a write pointer. The good thing is that the reads don't
/// cause retries in writes, unlike if it was just a single `TVar`
/// with one data structure in it. It may also help that it's more
/// granular, and `Transaction::downcast` will not clone a full
/// data structure.
#[derive(Clone)]
pub struct TChan<T> {
    read: TVar<TVarList<T>>,
    write: TVar<TVarList<T>>,
}

impl<T> TChan<T>
where
    T: Any + Sync + Send + Clone,
{
    /// Create an empty unbounded `TChan`.
    ///
    /// Both read and write `TVar`s will be pointing at a common `TVar`
    /// containing an empty list.
    /// ```no_run
    ///    [TNil]
    ///   / \
    /// [*]  [*]
    /// read write
    /// ```
    #[allow(dead_code)]
    pub fn new() -> TChan<T> {
        let hole = TVar::new(TList::TNil);
        TChan {
            read: TVar::new(hole.clone()),
            write: TVar::new(hole),
        }
    }
}

impl<T> TQueueLike<T> for TChan<T>
where
    T: Any + Sync + Send + Clone,
{
    /// Pop the head of the queue, or retry until there is an element if it's empty.
    ///
    /// Moves the read `TVar` down the list to point at the next item.
    /// ```no_run
    ///  [TCons(x, [TCons(y, [TNil])])]
    ///  |         |         |
    /// [ ]       [*]       [*]
    /// read0 ->  read1     write
    /// ```
    fn read(&self, transaction: &mut Transaction) -> StmResult<T> {
        let var_list = self.read.read(transaction)?;
        let list = var_list.read(transaction)?;
        match list {
            TList::TNil => retry(),
            TList::TCons(x, tail) => {
                self.read.write(transaction, tail)?;
                Ok(x)
            }
        }
    }

    /// Push to the end of the queue.
    ///
    /// Replaces the contents of the current write `TVar` with a `TCons` and points
    /// the write `TVar` at a new `TNil`.
    /// ```no_run
    ///  [TCons(x, [TCons(y, [TNil])])]
    ///  |         |         |
    /// [*]       [ ]       [*]
    /// read      write0 -> write1
    /// ```
    fn write(&self, transaction: &mut Transaction, value: T) -> StmResult<()> {
        let new_list_end = TVar::new(TList::TNil);
        let var_list = self.write.read(transaction)?;
        var_list.write(transaction, TList::TCons(value, new_list_end.clone()))?;
        self.write.write(transaction, new_list_end)?;
        Ok(())
    }
}

#[cfg(test)]
mod test_tqueuelike {
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

#[cfg(test)]
mod test_tchan {
    use super::TChan;
    use crate::tqueue::test_tqueuelike as tq;
    use etest::Bencher;

    #[test]
    fn write_and_read_back() {
        tq::write_and_read_back(TChan::<i32>::new());
    }

    #[test]
    fn threaded() {
        tq::threaded(TChan::<i32>::new());
    }

    #[bench]
    fn bench_two_threads_read_write(b: &mut Bencher) {
        tq::bench_two_threads_read_write(b, || TChan::<i32>::new());
    }

    #[bench]
    fn bench_one_thread_write_many_then_read(b: &mut Bencher) {
        tq::bench_one_thread_write_many_then_read(b, || TChan::<i32>::new());
    }

    #[bench]
    fn bench_one_thread_repeat_write_read(b: &mut Bencher) {
        tq::bench_one_thread_repeat_write_read(b, || TChan::<i32>::new());
    }
}
