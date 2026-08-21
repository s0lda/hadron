use std::path::PathBuf;
use std::time::Duration;
use hadron_lattice::locks::IntentLockTable;
use hadron_lattice::QuarkId;

#[test]
fn test_concurrent_write_detection_and_orthogonal_locks() {
    let mut table = IntentLockTable::new();
    let q1 = QuarkId::new("worker-1");
    let q2 = QuarkId::new("worker-2");

    let p1 = PathBuf::from("src/engine.rs");
    let p2 = PathBuf::from("src/router.rs");
    let p3 = PathBuf::from("src/lattice.rs");

    // Worker 1 acquires lock on p1 and p2
    let lease1 = table
        .try_acquire(q1.clone(), &[p1.clone(), p2.clone()], Duration::from_secs(30))
        .expect("Worker 1 should acquire locks cleanly");

    // Worker 2 attempts to acquire p2 and p3 -> should fail with conflict on p2
    let err = table
        .try_acquire(q2.clone(), &[p2.clone(), p3.clone()], Duration::from_secs(30))
        .unwrap_err();
    assert_eq!(err, vec![p2.clone()]);

    // Worker 2 acquires non-conflicting orthogonal path p3 -> should succeed
    let lease2 = table
        .try_acquire(q2.clone(), &[p3.clone()], Duration::from_secs(30))
        .expect("Worker 2 should acquire orthogonal lock on p3");

    // Worker 1 releases its lease
    table.release(&lease1);

    // Now Worker 2 can acquire p1 and p2
    let lease3 = table
        .try_acquire(q2.clone(), &[p1.clone(), p2.clone()], Duration::from_secs(30))
        .expect("Worker 2 should acquire p1 and p2 after release");

    table.release(&lease2);
    table.release(&lease3);
}
