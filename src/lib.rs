

// need to be replaced by a proper semaphore implementation
// or other alternative so that is compiles on stable
#![feature(semaphore)]

#[macro_use]
mod macros;
mod stm;
mod log;
mod var;

pub use stm::{STM, StmResult, retry};

pub use var::{Var};


#[test]
fn test_stm_macro() {
    let var = Var::new(0);

    let var2 = var.clone();

    let stm = stm!({
        var2.write(42);
        0
    });

    stm.atomically();
}



#[test]
fn test_stm_nested() {
    let var = Var::new(0);

    let var_ref = var.clone();

    let inner_stm = stm!({
        var_ref.write(42);
    });


    let stm = stm!({
        stm_call!(inner_stm);
        var.read()
    });

    let x = stm.atomically();
    assert_eq!(42, x);
}


#[test]
fn test_threaded() {
    use std::thread;
    use std::sync::mpsc::channel;

    let var = Var::new(0);

    let (tx, rx) = channel();

    let var_ref = var.clone();
    thread::spawn(move || {
        let stm = stm!({
            let x = var_ref.read();
            if x==0 {
                stm_call!(retry());
            }
            x
        });

        let _ = tx.send(stm.atomically());
    });

    thread::sleep_ms(100);

    stm!({
        var.write(42);
    }).atomically();

    let x = rx.recv().unwrap();

    assert_eq!(42, x);
}
