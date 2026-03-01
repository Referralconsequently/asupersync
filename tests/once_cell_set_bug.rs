//! Test for once cell set bug.
use asupersync::sync::OnceCell;
use std::sync::Arc;
use std::thread;
use std::time::Duration;

#[test]
fn test_once_cell_set_while_initializing() {
    let cell = Arc::new(OnceCell::<u32>::new());

    // Thread 1: Start initializing, but take a while
    let cell_clone = Arc::clone(&cell);
    let handle = thread::spawn(move || {
        let res = cell_clone.get_or_init_blocking(|| {
            thread::sleep(Duration::from_millis(50));
            42
        });
        assert_eq!(*res, 42);
    });

    // Give Thread 1 time to enter INITIALIZING state
    thread::sleep(Duration::from_millis(10));

    // Thread 2: Try to set. It will BLOCK until Thread 1 finishes.
    // Since Thread 1 succeeds, set will return Err(99).
    let set_result = cell.set(99);
    assert_eq!(
        set_result,
        Err(99),
        "set should return Err because thread 1 succeeded"
    );

    // get() will safely return Some(42).
    let get_result = cell.get();
    assert_eq!(get_result, Some(&42));

    handle.join().unwrap();
}

#[test]
fn test_once_cell_set_while_initializing_cancelled() {
    let cell = Arc::new(OnceCell::<u32>::new());

    // Thread 1: Start initializing, but panic (simulate cancellation)
    let cell_clone = Arc::clone(&cell);
    let handle = thread::spawn(move || {
        let _ = std::panic::catch_unwind(|| {
            cell_clone.get_or_init_blocking(|| {
                thread::sleep(Duration::from_millis(50));
                panic!("cancelled");
            });
        });
    });

    thread::sleep(Duration::from_millis(10));

    // Thread 2: Try to set. It will BLOCK until Thread 1 cancels.
    // Since Thread 1 cancels, state goes to UNINIT, and set(99) succeeds.
    let set_result = cell.set(99);
    assert_eq!(set_result, Ok(()), "set should succeed after cancellation");

    handle.join().unwrap();

    // The cell successfully stores 99 with no data loss.
    assert_eq!(cell.get(), Some(&99), "cell should contain 99");
}
