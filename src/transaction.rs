use std::collections::{BTreeSet,BTreeMap};


enum TransactionState<T> {
    /// the transaction is currently running
    Running,
    /// the transaction has finished with a given value
    Finished(T),

    /// the transaction has failed and will rerun as soon as
    /// possible
    Failed,

    /// explicit retry has been called causing the thread to
    /// block until something changes
    Retry,

    /// the thread has panicked
    Panicked,
}


type GenericState = TransactionState<Box<Any>>;


// currently unstable
use std::sync::Semaphore;


trait GenericTransaction {
    fn run(any: Box<Any>) -> GenericState;
}


#[must_use]
struct Transaction<T> {
    state: TransactionState<T>,
    written_vars: BTreeMap<Var, Box<Any>>,
    read_vars: BTreeSet<Var>,

    previous: Box<GenericTransaction>,
}


impl<T> Transaction<T> {
    /// monadic bind
    pub fn and_then<U, F>(self, f: F) -> Transaction<U> 
        where F : Fn(T) -> Transaction<U>
    {
        let Transaction{
            value: value,
            ..
        } = self;

        let boxed = Box::new()
        // apply calculation
        let next = f(value);

        // monadic join
    }

    /// join two monads into a single structure
    fn join<U>(self, other: mut Transaction<U>) -> Transaction<U> {
        let Transaction{
            written_vars: written_vars,
            read_vars: read_vars,
            ..
        } = self;

        // join the two sets
        other.read_vars.extend(read_vars.into_iter());

        // join the two maps and keep the newest value (that in other)
        for (k, v) in written_vars {
            other.written_vars.entry(k).or_insert(v);
        }

        // return the updated other
        other
    }


    fn read<U>(self, var: Var<U>) -> U {
        match self.write_var().get(var) {
            Some(v) => v,
            None    => {
                read_vars.insert(var);
                let v = var.read();
            }
        }
    }


    fn run(&mut self) {
        
    }

    /// run the computation again without blocking first
    fn retry_now(&mut self) {
        
    }


    pub fn wait_for_retry(&mut self) {
        let semaphore = Arc::new(Semaphore::new(1));
        // wait for all read vars
        for var in &mut self.read_vars {
            var.wait(semaphore);
        }
        // block until a variable changes
        semaphore.aquire();
    }
}



impl<T> GenericTransaction for Transaction<T> {
}
