use stm_core::*;
use std::any::Any;
use super::Queue;

/// `Queue` is a threadsafe FIFO queue, that uses software transactional memory.
///
/// It is similar to synchronous channels, but undoes operations 
/// in case of aborted transactions.
///
///
/// # Example
///
/// ```
/// extern crate stm;
///
/// use stm::*;
/// use stm::collections::BoundedQueue;
///
/// fn main() {
/// let queue = BoundedQueue::new(10);
/// let x = atomically(|tx| {
///     queue.push(tx, 42)?;
///     queue.pop(tx)
/// });
/// assert_eq!(x, 42);
/// }
/// ```
///
/// The implementation of `BoundedQueue` also serves as an example for the
/// composability of stm.
/// The whole bounded queue consists of an unbounded queue and a capacity.
/// ```
/// # use stm::*;
/// use stm::collections::Queue;
/// struct BoundedQueue<T> {
///     queue: Queue<T>,
///     cap: TVar<usize>,
/// }
/// ```
///
/// Popping an element is simple:
/// ```
/// # use stm::*;
/// # use stm::collections::Queue;
/// # struct BoundedQueue<T> {
/// #    queue: Queue<T>,
/// #    cap: TVar<usize>,
/// # }
///
/// pub fn pop(queue: BoundedQueue<String>, tx: &mut Transaction) -> StmResult<String> {
///     queue.cap.modify(tx, |x| x + 1)?;
///     queue.queue.pop(tx)
/// }
/// ```
///
/// Either none of these actions is committed of both of them.
/// If pop fails, modify is not committed and does not need to be undone manually.
///
///
#[derive(Clone)]
pub struct BoundedQueue<T> {
    /// Internally use a normal queue.
    queue: Queue<T>,

    /// `cap` stores the number of elements, that may still
    /// fit into this queue.
    cap: TVar<usize>,
}


impl<T: Any + Sync + Clone + Send> BoundedQueue<T> {
    /// Create new `BoundedQueue`, that can hold maximally
    /// `capacity` elements.
    pub fn new(capacity: usize) -> BoundedQueue<T> {
        BoundedQueue {
            queue: Queue::new(),
            cap: TVar::new(capacity),
        }
    }

    /// Add a new element to the queue.
    pub fn push(&self, tx: &mut Transaction, val: T) -> StmResult<()> {
        let cap = self.cap.read(tx)?;
        guard(cap > 0)?;
        self.cap.write(tx, cap - 1)?;
        self.queue.push(tx, val)
    }

    /// Push a value to the front of the queue. Next call to `pop` will return `value`.
    ///
    /// `push_front` allows to undo pop-operations and operates the queue in a LIFO way.
    pub fn push_front(&self, tx: &mut Transaction, value: T) -> StmResult<()> {
        let cap = self.cap.read(tx)?;
        guard(cap > 0)?;
        self.cap.write(tx, cap - 1)?;
        self.queue.push_front(tx, value)
    }

    /// Return the first element without removing it.
    pub fn try_peek(&self, tx: &mut Transaction) -> StmResult<Option<T>> {
        self.queue.try_peek(tx)
    }

    /// Return the first element without removing it.
    pub fn peek(&self, tx: &mut Transaction) -> StmResult<T> {
        self.queue.peek(tx)
    }

    /// Remove an element from the queue.
    pub fn try_pop(&self, tx: &mut Transaction) -> StmResult<Option<T>> {
        let v = self.queue.try_pop(tx)?;
        if v.is_some() {
            self.cap.modify(tx, |x| x + 1)?;
        }
        Ok(v)
    }

    /// Remove an element from the queue.
    pub fn pop(&self, tx: &mut Transaction) -> StmResult<T> {
        self.cap.modify(tx, |x| x + 1)?;
        self.queue.pop(tx)
    }

    /// Check if a queue is empty.
    pub fn is_empty(&self, tx: &mut Transaction) -> StmResult<bool> {
        self.queue.is_empty(tx)
    }

    /// Check if a queue is full.
    pub fn is_full(&self, tx: &mut Transaction) -> StmResult<bool> {
        let cap = self.cap.read(tx)?;
        Ok(cap == 0)
    }
}


#[cfg(test)]
mod tests {
    use stm_core::*;
    use super::*;

    /// Test if push and pop works and returns the right value.
    #[test]
    fn bqueue_push_pop() {
        let queue = BoundedQueue::new(1);
        let x = atomically(|tx| {
            queue.push(tx, 42)?;
            queue.pop(tx)
        });
        assert_eq!(42, x);
    }

    /// Test if push and pop operations maintain the order (FIFO).
    #[test]
    fn bqueue_order() {
        let queue = BoundedQueue::new(3);
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
    fn bqueue_multi_transactions() {
        let queue = BoundedQueue::new(3);
        let queue2 = queue.clone();

        atomically(|tx| {
            queue2.push(tx, 1)?;
            queue2.push(tx, 2)
        });
        atomically(|tx| queue.push(tx, 3));

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
    fn bqueue_threaded() {
        use std::thread;
        let queue = BoundedQueue::new(10);

        // Spawn 10 threads, that write to the queue.
        for i in 0..10 {
            let queue2 = queue.clone();
            thread::spawn(move || { atomically(|tx| queue2.push(tx, i)) });
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

    /// Just like `bqueue_threaded`, but the
    /// queue is too short to hold all elements simultaneously.
    ///
    /// The threads must push the elments one after another.
    /// The main thread has to block multiple times while querying
    /// all elements.
    #[test]
    fn bqueue_threaded_short_queue() {
        use std::thread;
        let queue = BoundedQueue::new(2);

        for i in 0..10 {
            let queue2 = queue.clone();
            thread::spawn(move || { atomically(|tx| queue2.push(tx, i)); });
        }

        // Wait for all the values.
        //
        // We can not read all the values in one transaction and therefore need 
        // multiple reads.
        let mut v = Vec::new();
        for _ in 0..10 {
            v.push(atomically(|tx| queue.pop(tx)));
        }

        // We don't know the order, but want to check if we received everything
        // correcty.
        v.sort();
        for i in 0..10 {
            assert_eq!(v[i], i);
        }
    }
}

