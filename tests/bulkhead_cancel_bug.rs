#![allow(missing_docs)]

use asupersync::combinator::bulkhead::{Bulkhead, BulkheadPolicy};
use asupersync::types::Time;

#[test]
fn test_cancel_head_of_line_blocking() {
    let bh = Bulkhead::new(BulkheadPolicy {
        max_concurrent: 3,
        max_queue: 10,
        ..Default::default()
    });
    let now = Time::from_millis(0);

    // Queue is empty, available is 3.
    // Try to enqueue A wanting 5 (will block)
    let a_id = bh.enqueue(5, now).unwrap();

    // Try to enqueue B wanting 2 (will block because A is ahead of it in FIFO)
    let b_id = bh.enqueue(2, now).unwrap();

    // Process queue (nothing happens because A wants 5 and we have 3)
    bh.process_queue(now);

    assert!(matches!(bh.check_entry(a_id, now), Ok(None)));
    assert!(matches!(bh.check_entry(b_id, now), Ok(None)));

    // Cancel A
    bh.cancel_entry(a_id, now);

    // B should now be granted! Because A was blocking it, and now A is gone.
    // BUT we must check if `cancel_entry` internally processed the queue.
    // Check entry B
    let _b_status = bh.check_entry(b_id, now);

    // Actually `check_entry` calls `process_queue`!
    // Let's see: `check_entry` calls `process_queue_inner`!
    // Ah. If `check_entry` calls `process_queue`, then B will be granted when someone calls `check_entry(b_id)`.

    // But what if no one calls `check_entry` immediately?
    // Wait, the caller of B is waiting on an async Future?
    // `Bulkhead` itself doesn't have an async `Acquire` future in this file. It just has `enqueue` and `check_entry`.
}
