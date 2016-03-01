use std::thread;
use std::time::Duration;
use std::sync::mpsc::channel;

/// Check if a function `f` terminates within a given timeframe.
///
/// It is used to check for deadlocks.
///
/// If the function does not terminate, it keeps a thread alive forever,
/// so don't run too many test (preferable just one) in sequence.
pub fn terminates<F>(duration_ms: u64, f: F) -> bool 
where F: Send + FnOnce() -> () + 'static,
{
    terminates_async(duration_ms, f, || {})
}

pub fn terminates_async<F, F2>(duration_ms: u64, f: F, f2: F2) -> bool 
where F: Send + FnOnce() -> () + 'static,
      F2: FnOnce() -> ()
{
    async(duration_ms, f, f2).is_some()
}

pub fn async<T, F, F2>(duration_ms: u64, f: F, f2: F2) -> Option<T>
where F: Send + FnOnce() -> T + 'static,
      F2: FnOnce() -> (),
      T: Send + 'static,
{
    let (tx, rx) = channel();

    thread::spawn(move || {
        let t = f();
        // wakeup other thread
        let _ = tx.send(t);
    });
    
    f2();
    
    if let a@Some(_) = rx.try_recv().ok() {
        return a;
    }

    for _ in 0..duration_ms/50 {
        thread::sleep(Duration::from_millis(50));
        if let a@Some(_) = rx.try_recv().ok() {
            return a;
        }
    }

    thread::sleep(Duration::from_millis(duration_ms % 50));

    rx.try_recv().ok()
}
