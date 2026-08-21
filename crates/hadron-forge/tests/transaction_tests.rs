use std::fs;
use hadron_forge::transaction::{BatchEditTransaction, FileEditOp};
use tempfile::tempdir;

#[test]
fn test_batch_transaction_atomic_rollback_on_hash_mismatch() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let f1 = root.join("f1.txt");
    let f2 = root.join("f2.txt");
    let f3 = root.join("f3.txt");

    fs::write(&f1, "original f1").unwrap();
    fs::write(&f2, "original f2").unwrap();
    fs::write(&f3, "original f3").unwrap();

    let h1 = BatchEditTransaction::hash_content("original f1");
    let h2 = BatchEditTransaction::hash_content("original f2");
    let bad_h3 = "0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef"; // stale/mismatched

    let mut tx = BatchEditTransaction::new();
    tx.add_edit(FileEditOp {
        path: f1.clone(),
        expected_hash: Some(h1),
        new_content: "modified f1".into(),
    });
    tx.add_edit(FileEditOp {
        path: f2.clone(),
        expected_hash: Some(h2),
        new_content: "modified f2".into(),
    });
    tx.add_edit(FileEditOp {
        path: f3.clone(),
        expected_hash: Some(bad_h3.into()),
        new_content: "modified f3".into(),
    });

    let res = tx.validate_and_apply();
    assert!(res.is_err(), "Transaction must fail due to hash mismatch on f3");

    // Assert atomic rollback: no files modified
    assert_eq!(fs::read_to_string(&f1).unwrap(), "original f1");
    assert_eq!(fs::read_to_string(&f2).unwrap(), "original f2");
    assert_eq!(fs::read_to_string(&f3).unwrap(), "original f3");
}

#[test]
fn test_batch_transaction_success() {
    let dir = tempdir().unwrap();
    let root = dir.path();

    let f1 = root.join("f1.txt");
    let f2 = root.join("f2.txt");

    fs::write(&f1, "hello").unwrap();
    fs::write(&f2, "world").unwrap();

    let h1 = BatchEditTransaction::hash_content("hello");
    let h2 = BatchEditTransaction::hash_content("world");

    let mut tx = BatchEditTransaction::new();
    tx.add_edit(FileEditOp {
        path: f1.clone(),
        expected_hash: Some(h1),
        new_content: "hello updated".into(),
    });
    tx.add_edit(FileEditOp {
        path: f2.clone(),
        expected_hash: Some(h2),
        new_content: "world updated".into(),
    });

    let report = tx.validate_and_apply().expect("Transaction should succeed");
    assert_eq!(report.files_modified.len(), 2);
    assert_eq!(fs::read_to_string(&f1).unwrap(), "hello updated");
    assert_eq!(fs::read_to_string(&f2).unwrap(), "world updated");
}
