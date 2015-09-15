use std::sync::{Arc, Semaphore};


#[derive(PartialEq, Eq)]
pub enum VarVersion(u64);


pub struct InnerVar<T> {
    value: T,
    waiting_threads: Mutex<Vec<Arc<Semaphore>>>
}

pub struct Var<T> {
    inner: Arc<InnerVar<T>>
}


type VarID = usize;

impl<T> Var<T> {
    pub fn new() -> Transaction<Var<T>> {
    }

    pub fn read(&self) -> T {
        value
    }

    pub fn write(&mut self, val: T) -> Transaction<()> {
        self.value = Some(val);
    }

    pub fn var_id(&self) -> usize {
        &*self.inner as const* T as usize
    }


    fn wake_all(&mut self) -> usize {
        // atomically take all waiting threads from the value
        let threads = {
            let guard = self.waiting_threads.lock().unwrap();
            mem::replace(&mut guard, Vec::new());
        };

        // release all the semaphores to start the thread
        for thread in threads {
            thread.release();
        }
    }

    fn wait(&mut self, thread: Arc<Semaphore>) {
        let guard = self.waiting_threads.lock().unwrap();
        guard.push(thread);
    }
}


