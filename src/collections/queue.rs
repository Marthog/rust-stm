use stm_core::*;
use std::any::Any;
use super::ArcList;

// Queue is implemented using two lists (`read` and `write`).
// `push` writes to the beginning of `write` and `pop` reads from the
// beginning of `read`. If `read` is empty, the reversed list `write` is
// used as a new list. This way all operations are amortized constant time.

/// `Queue` is a threadsafe FIFO queue.
///
/// It is similar to channels, but undoes operations in 
/// case of aborted transactions.
///
///
/// # Example
///
/// ```
/// extern crate stm;
///
/// use stm::*;
/// use stm::collections::Queue;
///
/// fn main() {
///     let queue = Queue::new();
///     let x = atomically(|tx| {
///         queue.push(tx, 42)?;
///         queue.pop(tx)
///     });
///     assert_eq!(x, 42);
/// }
/// ```
#[derive(Clone)]
pub struct Queue<T> {
    read: TVar<ArcList<T>>,
    write: TVar<ArcList<T>>,
}

impl<T: Any + Sync + Clone + Send> Queue<T> {
    /// Create a new queue.
    pub fn new() -> Queue<T> {
        Queue {
            read: TVar::new(ArcList::new()),
            write: TVar::new(ArcList::new()),
        }
    }

    /// Add a new element to the queue.
    pub fn push(&self, tx: &mut Transaction, value: T) -> StmResult<()> {
        self.write.modify(tx, |end| end.prepend(value))
    }

    /// Push a value to the front of the queue. Next call to `pop` will return `value`.
    ///
    /// `push_front` allows to undo pop-operations and operates the queue in a LIFO way.
    pub fn push_front(&self, tx: &mut Transaction, value: T) -> StmResult<()> {
        self.read.modify(tx, |end| end.prepend(value))
    }

    /// Return the first element without removing it.
    pub fn try_peek(&self, tx: &mut Transaction) -> StmResult<Option<T>> {
        // Use try_pop here, because try_pop contains the whole queue-reversing code.
        let v = self.try_pop(tx)?;
        if let Some(ref e) = v {
            self.push_front(tx, e.clone())?;
        }
        Ok(v)
    }

    /// Return the first element without removing it.
    pub fn peek(&self, tx: &mut Transaction) -> StmResult<T> {
        let v = self.pop(tx)?;
        self.push_front(tx, v.clone())?;
        Ok(v)
    }

    /// Remove an element from the queue.
    pub fn try_pop(&self, tx: &mut Transaction) -> StmResult<Option<T>> {
        Ok(match self.read.read(tx)?.into_splitted() {
            Some((x, xs)) => {
                self.read.write(tx, xs)?;
                Some(x)
            }
            None => {
                let write_list = self.write.replace(tx, ArcList::new())?;
                match write_list.reverse().into_splitted() {
                    None => None,
                    Some((x, xs)) => {
                        self.read.write(tx, xs.clone())?;
                        Some(x)
                    }
                }
            }
        })
    }

    /// Remove an element from the queue.
    pub fn pop(&self, tx: &mut Transaction) -> StmResult<T> {
        unwrap_or_retry(self.try_pop(tx)?)
    }

    /// Check if a queue is empty.
    pub fn is_empty(&self, tx: &mut Transaction) -> StmResult<bool> {
        Ok(
            self.read.read(tx)?.is_empty() && self.write.read(tx)?.is_empty(),
        )
    }
}


#[cfg(test)]
mod tests {
    use stm_core::*;
    use super::*;

    /// Check if push and pop returns the right value.
    #[test]
    fn channel_push_pop() {
        let queue = Queue::new();
        let x = atomically(|tx| {
            queue.push(tx, 42)?;
            queue.pop(tx)
        });
        assert_eq!(42, x);
    }

    /// Check if the queue works as a FIFO within a single transaction.
    #[test]
    fn channel_order() {
        let queue = Queue::new();
        let x = atomically(|tx| {
            queue.push(tx, 1)?;
            queue.push(tx, 2)?;
            queue.push(tx, 3)?;
            let x1 = queue.pop(tx)?;
            let x2 = queue.pop(tx)?;
            let x3 = queue.pop(tx)?;
            Ok((x1, x2, x3))
        });
        assert_eq!((1, 2, 3), x);
    }

    /// Check if the queue on multiple consecutive transactions.
    ///
    /// Basically this checks if everything is committed correctly.
    #[test]
    fn channel_multi_transactions() {
        let queue = Queue::new();
        let queue2 = queue.clone();

        // First push some values.
        atomically(|tx| {
            queue2.push(tx, 1)?;
            queue2.push(tx, 2)
        });
        atomically(|tx| queue.push(tx, 3));

        // Get the values and check for consistency.
        let x = atomically(|tx| {
            let x1 = queue.pop(tx)?;
            let x2 = queue.pop(tx)?;
            let x3 = queue.pop(tx)?;
            Ok((x1, x2, x3))
        });
        assert_eq!((1, 2, 3), x);
    }

    /// Test if the queue works with multiple concurrent threads.
    #[test]
    fn channel_threaded() {
        use std::thread;
        use std::time::Duration;
        let queue = Queue::new();

        // Spawn 10 threads, that write to the queue.
        for i in 0..10 {
            let queue2 = queue.clone();
            thread::spawn(move || {
                thread::sleep(Duration::from_millis(20 as u64));
                atomically(|tx| queue2.push(tx, i));
            });
        }

        // Wait for all the values.
        let mut v = atomically(|tx| {
            let mut v = Vec::new();
            for _ in 0..10 {
                v.push(queue.pop(tx)?);
            }
            Ok(v)
        });

        // We don't know the order, but want to check if we received everything
        // correcty.
        v.sort();
        for i in 0..10 {
            assert_eq!(v[i], i);
        }
    }
}

