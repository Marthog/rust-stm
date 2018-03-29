//! This module contains helpers for various tests.
//! Quite a lot of tests run operations asynchonously and need to check 
//! for deadlocks. We do this by waiting a certain amount of time for completion.
//!
//! This module contains some helpers that simplify other tests.

use std::thread;
use std::time::Duration;
use std::sync::mpsc::channel;

/// Check if a function `f` terminates within a given timeframe.
///
/// If the function does not terminate, it keeps a thread alive forever,
/// so don't run too many test (preferable just one) in sequence.
pub fn terminates<F>(duration_ms: u64, f: F) -> bool 
where F: Send + FnOnce() -> () + 'static,
{
    terminates_async(duration_ms, f, || {})
}

/// Check if a function `f` terminates within a given timeframe, 
/// but run a second function `g` concurrently.
///
/// If the function does not terminate, it keeps a thread alive forever,
/// so don't run too many test (preferable just one) in sequence.
pub fn terminates_async<F, G>(duration_ms: u64, f: F, g: G) -> bool 
where F: Send + FnOnce() -> () + 'static,
      G: FnOnce() -> ()
{
    async(duration_ms, f, g).is_some()
}

/// Run two functions `f` and `g` concurrently.
///
/// Run `f` in a second thread, `g` in the main thread. Wait the given time `duration_ms` for `g`
/// and return `f`s return value or return `None` if `f` does not terminate.
///
/// If `f` does not terminate, it keeps a thread alive forever,
/// so don't run too many test (preferable just one) in sequence.
pub fn async<T, F, G>(duration_ms: u64, f: F, g: G) -> Option<T>
where F: Send + FnOnce() -> T + 'static,
      G: FnOnce() -> (),
      T: Send + 'static,
{
    let (tx, rx) = channel();

    thread::spawn(move || {
        let t = f();
        // wakeup other thread
        let _ = tx.send(t);
    });
    
    g();
    
    if let a@Some(_) = rx.try_recv().ok() {
        return a;
    }

    // Give enough time for travis to get up.
    // Sleep in 50 ms steps, so that it does not waste too much time if the thread finishes earlier.
    for _ in 0..duration_ms/50 {
        thread::sleep(Duration::from_millis(50));
        if let a@Some(_) = rx.try_recv().ok() {
            return a;
        }
    }

    thread::sleep(Duration::from_millis(duration_ms % 50));

    rx.try_recv().ok()
}
