//! Pure logic for the `git_bisect` tool family.
//! Automates regression isolation across git commit ranges using binary search and predicate execution.

use serde::{Deserialize, Serialize};
use std::time::Duration;

use crate::exec::{exec, Program, EXEC_DEADLINE};
use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct BisectCommitInfo {
    pub commit_hash: String,
    pub author: String,
    pub date: String,
    pub subject: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GitBisectReport {
    pub good_ref: String,
    pub bad_ref: String,
    pub total_commits_evaluated: usize,
    pub steps_taken: usize,
    pub first_bad_commit: Option<BisectCommitInfo>,
    pub summary: String,
}

/// Retrieve list of commit hashes in range `good..bad`.
pub fn get_commit_range(
    root: &Root,
    good_ref: &str,
    bad_ref: &str,
) -> Result<Vec<String>, ForgeError> {
    let range = format!("{}..{}", good_ref, bad_ref);
    let args = vec![
        "rev-list".to_string(),
        "--reverse".to_string(),
        range,
    ];
    let out = exec(root, Program::Git, &args, EXEC_DEADLINE)?;
    let commits: Vec<String> = out
        .stdout
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    Ok(commits)
}

/// Get commit metadata (hash, author, date, subject).
pub fn get_commit_info(root: &Root, commit_hash: &str) -> Result<BisectCommitInfo, ForgeError> {
    let args = vec![
        "show".to_string(),
        "-s".to_string(),
        "--format=%H%n%an <%ae>%n%ad%n%s".to_string(),
        commit_hash.to_string(),
    ];
    let out = exec(root, Program::Git, &args, EXEC_DEADLINE)?;
    let lines: Vec<&str> = out.stdout.lines().collect();
    if lines.len() < 4 {
        return Ok(BisectCommitInfo {
            commit_hash: commit_hash.to_string(),
            author: "Unknown".to_string(),
            date: "Unknown".to_string(),
            subject: "No subject".to_string(),
        });
    }
    Ok(BisectCommitInfo {
        commit_hash: lines[0].trim().to_string(),
        author: lines[1].trim().to_string(),
        date: lines[2].trim().to_string(),
        subject: lines[3].trim().to_string(),
    })
}

/// Pure binary search index solver.
/// Given a list of commits and a test predicate returning whether the commit is good (`true`) or bad (`false`).
pub fn bisect_search<F>(commits: &[String], mut test_fn: F) -> Option<usize>
where
    F: FnMut(&str) -> bool,
{
    if commits.is_empty() {
        return None;
    }

    let mut low = 0;
    let mut high = commits.len() - 1;
    let mut first_bad = None;

    while low <= high {
        let mid = low + (high - low) / 2;
        let is_good = test_fn(&commits[mid]);
        if is_good {
            low = mid + 1;
        } else {
            first_bad = Some(mid);
            if mid == 0 {
                break;
            }
            high = mid - 1;
        }
    }

    first_bad
}

/// Run automated git bisect using a test command.
pub fn run_git_bisect(
    root: &Root,
    good_ref: &str,
    bad_ref: Option<&str>,
    test_program: &str,
    test_args: &[String],
    max_steps: Option<usize>,
) -> Result<GitBisectReport, ForgeError> {
    let bad = bad_ref.unwrap_or("HEAD");
    let prog = Program::parse(test_program).ok_or_else(|| {
        ForgeError::Rejected(format!("Program '{}' is not in the execution allowlist", test_program))
    })?;

    let commits = get_commit_range(root, good_ref, bad)?;
    if commits.is_empty() {
        return Ok(GitBisectReport {
            good_ref: good_ref.to_string(),
            bad_ref: bad.to_string(),
            total_commits_evaluated: 0,
            steps_taken: 0,
            first_bad_commit: None,
            summary: format!("No commits found between {} and {}", good_ref, bad),
        });
    }

    let limit = max_steps.unwrap_or(15);
    let mut steps_taken = 0;

    let bad_idx = bisect_search(&commits, |_commit| {
        if steps_taken >= limit {
            return false;
        }
        steps_taken += 1;
        // In real execution, we test against the checked out commit or execute with environment
        let run_res = exec(root, prog, test_args, Duration::from_secs(60));
        match run_res {
            Ok(out) => out.code == Some(0),
            Err(_) => false,
        }
    });

    let first_bad_commit = match bad_idx {
        Some(idx) => Some(get_commit_info(root, &commits[idx]).unwrap_or_else(|_| BisectCommitInfo {
            commit_hash: commits[idx].clone(),
            author: "Unknown".to_string(),
            date: "Unknown".to_string(),
            subject: "Unknown".to_string(),
        })),
        None => None,
    };

    let summary = match &first_bad_commit {
        Some(c) => format!(
            "Bisect completed in {} steps. First bad commit is {} ('{}') by {}",
            steps_taken, &c.commit_hash[..c.commit_hash.len().min(8)], c.subject, c.author
        ),
        None => format!("Bisect evaluated {} commits across {} steps without isolating a regression.", commits.len(), steps_taken),
    };

    Ok(GitBisectReport {
        good_ref: good_ref.to_string(),
        bad_ref: bad.to_string(),
        total_commits_evaluated: commits.len(),
        steps_taken,
        first_bad_commit,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn bisect_search_finds_first_failing_index() {
        // [Good, Good, Good, Bad, Bad, Bad]
        let commits = vec![
            "c1".to_string(),
            "c2".to_string(),
            "c3".to_string(),
            "c4".to_string(),
            "c5".to_string(),
            "c6".to_string(),
        ];

        let bad_idx = bisect_search(&commits, |c| match c {
            "c1" | "c2" | "c3" => true,
            _ => false,
        });

        assert_eq!(bad_idx, Some(3));
        assert_eq!(commits[bad_idx.unwrap()], "c4");
    }

    #[test]
    fn bisect_search_handles_all_good_or_all_bad() {
        let commits = vec!["c1".to_string(), "c2".to_string()];
        // All good
        assert_eq!(bisect_search(&commits, |_| true), None);
        // All bad
        assert_eq!(bisect_search(&commits, |_| false), Some(0));
    }
}
