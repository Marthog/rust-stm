use stm_core::*;

/// `Semaphore` is an implementation of semaphores on top of software transactional
/// memory.
///
/// This is a very simple datastructure and serves as a simple thread
/// synchronization primitive.
#[derive(Clone, Debug)]
pub struct Semaphore {
    /// Semaphores are internally just a number.
    num: TVar<u32>,
}

impl Semaphore {
    /// Create a new semaphore with `n` initial tokens.
    pub fn new(n: u32) -> Semaphore {
        Semaphore { num: TVar::new(n) }
    }

    /// Take a token from the semaphore or retry if none left.
    pub fn wait(&self, tx: &mut Transaction) -> StmResult<()> {
        let n = self.num.read(tx)?;
        guard(n != 0)?;
        self.num.write(tx, n - 1)
    }

    /// Free a token.
    pub fn signal(&self, tx: &mut Transaction) -> StmResult<()> {
        self.num.modify(tx, |n| n + 1)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Test if wait with start value of 1 works.
    #[test]
    fn sem_wait() {
        let sem = Semaphore::new(1);
        atomically(|tx| sem.wait(tx));
    }

    /// Test if signal and wait combo works.
    #[test]
    fn sem_signal_wait() {
        let sem = Semaphore::new(0);
        atomically(|tx| {
            sem.signal(tx)?;
            sem.wait(tx)
        });
    }

    /// Test if the semaphore can be used to synchronize two threads.
    #[test]
    fn sem_threaded() {
        use std::thread;

        let sem = Semaphore::new(0);
        let sem2 = sem.clone();

        thread::spawn(move || for _ in 0..10 {
            atomically(|tx| sem2.signal(tx));
        });

        for _ in 0..10 {
            atomically(|tx| sem.wait(tx));
        }
    }

    /// Test if the semophore works with more than one thread.
    #[test]
    fn sem_threaded2() {
        use std::thread;

        let sem = Semaphore::new(0);

        for _ in 0..10 {
            let sem2 = sem.clone();
            thread::spawn(move || { atomically(|tx| sem2.signal(tx)); });
        }

        for _ in 0..10 {
            atomically(|tx| sem.wait(tx));
        }
    }
}
