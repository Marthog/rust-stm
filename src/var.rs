
use std::sync::{Arc, Semaphore, Mutex, RwLock};
use std::mem;
use std::sync::atomic::{self, AtomicUsize};
use std::cmp;
use std::any::Any;
use std::marker::PhantomData;

use super::stm::{with_log, StmControlBlock};


pub struct VarControlBlock {
    waiting_threads: Mutex<Vec<Arc<StmControlBlock>>>,
    dead_threads: AtomicUsize,
    pub value: RwLock<Arc<Any>>,
}


impl VarControlBlock {
    /// create a new empty `VarControlBlock`
    pub fn new<T>(val: T) -> Arc<VarControlBlock>
        where T: Any+Sync+Send
    {
        let ctrl = VarControlBlock {
            waiting_threads: Mutex::new(Vec::new()),
            dead_threads: AtomicUsize::new(0),
            value: RwLock::new(Arc::new(val) as Arc<Any>),
        };
        Arc::new(ctrl)
    }

    /// wake all threads that are waiting for the used var
    pub fn wake_all(&self) {
        // atomically take all waiting threads from the value
        let threads = {
            let mut guard = self.waiting_threads.lock().unwrap();
            let inner: &mut Vec<_> = &mut guard;
            mem::replace(inner, Vec::new())
        };

        // release all the semaphores to start the thread
        for thread in threads {
            // inform thread that this var has changed
            thread.set_changed();
        }
    }

    /// add another thread that waits for mutations of `self`
    pub fn wait(&self, thread: Arc<StmControlBlock>) {
        let mut guard = self.waiting_threads.lock().unwrap();
        
        // add new one
        guard.push(thread);
    }

    /// mark another `StmControlBlock` as dead
    ///
    /// when the count of dead control blocks is too high
    /// then perform a cleanup
    ///
    /// this prevents masses of old `StmControlBlock` to
    /// pile up when a variable is often read but not written
    pub fn set_dead(&self) {
        // increase by one
        let deads = self.dead_threads.fetch_add(1, atomic::Ordering::Relaxed);

        // if there are too many then cleanup
        
        // there is a potential data race that may occure when
        // one thread reads the number and then operates on
        // outdated data but that causes just unnecessary locks
        // to occur and nothing serious
        if deads>=64 {
            let mut guard = self.waiting_threads.lock().unwrap();
            self.dead_threads.store(0, atomic::Ordering::SeqCst);

            // remove all dead ones possibly free up the memory
            guard.retain(|t| !t.is_dead());
        }
    }

    fn get_address(&self) -> usize {
        self as *const VarControlBlock as usize
    }
}


// implement some operators so that VarControlBlocks can be sorted

impl PartialEq for VarControlBlock {
    fn eq(&self, other: &Self) -> bool {
        self.get_address()==other.get_address()
    }
}

impl Eq for VarControlBlock {
}

impl Ord for VarControlBlock {
    fn cmp(&self, other: &Self) -> cmp::Ordering {
        self.get_address().cmp(&other.get_address())
    }
}

impl PartialOrd for VarControlBlock {
    fn partial_cmp(&self, other: &Self) -> Option<cmp::Ordering> {
        Some(self.cmp(other))
    }
}



#[derive(Clone)]
pub struct Var<T> {
    control_block: Arc<VarControlBlock>,
    /// this marker is needed so that the variable can be used in a threadsafe
    /// manner
    _marker: PhantomData<T>
}

impl<T> Var<T>
    where T: Any+Sync+Send+Clone
{
    pub fn new(val: T) -> Var<T> {
        Var {
            control_block: VarControlBlock::new(val),
            _marker: PhantomData,
        }
    }

    pub fn read_immediate(&self) -> T {
        let val = self.control_block.value.read().unwrap();
        let val = val.downcast_ref::<T>();
        val.expect("wrong type in Var<T>").clone()
    }

    pub fn read_ref(&self) -> Arc<Any> {
        self.control_block.value.read().unwrap().clone()
    }

    pub fn read(&self) -> T {
        with_log(|log| log.read_var(self))
    }

    pub fn write(&self, value: T) {
        with_log(|log| log.write_var(self, value));
    }

    pub fn wake_all(&self) {
        self.control_block.wake_all();
    }

    pub fn control_block(&self) -> &Arc<VarControlBlock> {
        &self.control_block
    }
}



#[derive(PartialEq, Eq)]
pub enum VarVersion {
    VarVersion(u64)
}


#[test]
fn test_wait() {
    use std::thread;
    use std::sync::mpsc::channel;

    let ctrl = Arc::new(StmControlBlock::new());

    let var = Var::new(vec![1,2,3,4]);

    let (tx, rx) = channel();

    // add to list of blocked things
    var.control_block.wait(ctrl.clone());
    // wake me again
    var.wake_all();


    let ctrl2 = ctrl.clone();
    // there is no way to ensure that the semaphore has been unlocked
    let handle = thread::spawn(move || {
        // 300 ms should be enough, otherwise way to slow
        thread::sleep_ms(300);
        let err = rx.try_recv().is_err();
        ctrl2.set_changed();
        if err {
            panic!("semaphore not released before timeout");
        }
    });

    // get semaphore
    ctrl.wait();

    let _ = tx.send(());

    handle.join();
}

