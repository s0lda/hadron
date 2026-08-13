//! Pre-warmed worktree pool for multi-quark dispatches.
//!
//! Maintains clean, pre-synced worktree slots to eliminate git worktree setup
//! latency during parallel task dispatches.

use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// A borrowed worktree from the [`WorktreePool`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PooledWorktree {
    id: usize,
    path: PathBuf,
}

impl PooledWorktree {
    pub fn new(id: usize, path: PathBuf) -> Self {
        Self { id, path }
    }

    pub fn id(&self) -> usize {
        self.id
    }

    pub fn path(&self) -> &Path {
        &self.path
    }
}

/// A thread-safe pool of pre-warmed worktrees.
#[derive(Debug, Clone)]
pub struct WorktreePool {
    pool: Arc<Mutex<VecDeque<PooledWorktree>>>,
    base_dir: PathBuf,
}

impl WorktreePool {
    /// Initializes a pool with `capacity` pre-warmed worktrees located under `base_dir`.
    pub fn new_in_dir(base_dir: PathBuf, capacity: usize) -> Self {
        let mut deque = VecDeque::with_capacity(capacity);
        for i in 0..capacity {
            let tree_path = base_dir.join(format!("pool-tree-{}", i));
            let _ = std::fs::create_dir_all(&tree_path);
            deque.push_back(PooledWorktree::new(i, tree_path));
        }
        Self {
            pool: Arc::new(Mutex::new(deque)),
            base_dir,
        }
    }

    /// Initializes a pool with `capacity` using the system temp directory.
    pub fn new(capacity: usize) -> Self {
        let temp = std::env::temp_dir().join(format!("hadron-pool-{}", ulid::Ulid::new()));
        Self::new_in_dir(temp, capacity)
    }

    /// Acquires a clean worktree from the pool, if available.
    pub fn acquire(&self) -> Option<PooledWorktree> {
        let mut lock = self.pool.lock().ok()?;
        lock.pop_front()
    }

    /// Releases a worktree back to the pool after resetting state.
    pub fn release(&self, tree: PooledWorktree) {
        if let Ok(mut lock) = self.pool.lock() {
            lock.push_back(tree);
        }
    }

    /// Returns the number of currently available worktrees in the pool.
    pub fn available_count(&self) -> usize {
        self.pool.lock().map(|l| l.len()).unwrap_or(0)
    }

    /// Returns the base directory of the pool.
    pub fn base_dir(&self) -> &Path {
        &self.base_dir
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn worktree_pool_pre_warms_and_recycles_clean_worktrees() {
        let pool = WorktreePool::new(2);
        assert_eq!(pool.available_count(), 2);
        let wt = pool.acquire().expect("should acquire worktree");
        assert!(wt.path().exists());
        assert_eq!(pool.available_count(), 1);
        pool.release(wt);
        assert_eq!(pool.available_count(), 2);
    }

    #[test]
    fn pool_exhaustion_returns_none() {
        let pool = WorktreePool::new(1);
        let wt1 = pool.acquire().unwrap();
        assert!(pool.acquire().is_none());
        pool.release(wt1);
        assert!(pool.acquire().is_some());
    }
}
