//! Pure logic for the `release_sync` tool family.
//! Conventional commit analysis, automated SemVer bump computation, and changelog generation.

use serde::{Deserialize, Serialize};

use crate::exec::{exec, Program, EXEC_DEADLINE};
use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum SemVerBump {
    Major,
    Minor,
    Patch,
    None,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ConventionalCommit {
    pub hash: String,
    pub commit_type: String,
    pub scope: Option<String>,
    pub description: String,
    pub is_breaking: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ReleaseSyncReport {
    pub since_tag: String,
    pub total_commits: usize,
    pub recommended_bump: SemVerBump,
    pub recommended_version: Option<String>,
    pub changelog_snippet: String,
    pub summary: String,
}

/// Parse a commit message subject into conventional commit structure.
pub fn parse_conventional_commit(hash: &str, message: &str) -> ConventionalCommit {
    let trimmed = message.trim();
    let is_breaking = trimmed.contains("BREAKING CHANGE") || trimmed.contains("!:");

    let (prefix, description) = match trimmed.split_once(':') {
        Some((p, d)) => (p.trim(), d.trim()),
        None => ("chore", trimmed),
    };

    let (commit_type, scope) = if let Some(open) = prefix.find('(') {
        if let Some(close) = prefix.find(')') {
            let c_type = prefix[..open].trim();
            let scope_val = prefix[open + 1..close].trim();
            (c_type, Some(scope_val.to_string()))
        } else {
            (prefix, None)
        }
    } else {
        (prefix.trim_end_matches('!'), None)
    };

    ConventionalCommit {
        hash: hash.to_string(),
        commit_type: commit_type.to_lowercase(),
        scope,
        description: description.to_string(),
        is_breaking,
    }
}

/// Compute recommended SemVer bump from a collection of conventional commits.
pub fn determine_bump(commits: &[ConventionalCommit]) -> SemVerBump {
    let mut has_minor = false;
    let mut has_patch = false;

    for c in commits {
        if c.is_breaking {
            return SemVerBump::Major;
        }
        match c.commit_type.as_str() {
            "feat" => has_minor = true,
            "fix" | "perf" | "refactor" => has_patch = true,
            _ => {}
        }
    }

    if has_minor {
        SemVerBump::Minor
    } else if has_patch {
        SemVerBump::Patch
    } else if !commits.is_empty() {
        SemVerBump::Patch
    } else {
        SemVerBump::None
    }
}

/// Generate formatted CHANGELOG markdown from commits.
pub fn generate_changelog(commits: &[ConventionalCommit]) -> String {
    let mut features = Vec::new();
    let mut fixes = Vec::new();
    let mut refactors = Vec::new();
    let mut others = Vec::new();

    for c in commits {
        let short_hash = if c.hash.len() > 7 { &c.hash[..7] } else { &c.hash };
        let scope_str = c.scope.as_ref().map(|s| format!("**({})**: ", s)).unwrap_or_default();
        let entry = format!("- {}{}{} ({})", if c.is_breaking { "🚨 **BREAKING**: " } else { "" }, scope_str, c.description, short_hash);

        match c.commit_type.as_str() {
            "feat" => features.push(entry),
            "fix" => fixes.push(entry),
            "refactor" | "perf" => refactors.push(entry),
            _ => others.push(entry),
        }
    }

    let mut out = String::new();
    if !features.is_empty() {
        out.push_str("### Features\n\n");
        out.push_str(&features.join("\n"));
        out.push_str("\n\n");
    }
    if !fixes.is_empty() {
        out.push_str("### Bug Fixes\n\n");
        out.push_str(&fixes.join("\n"));
        out.push_str("\n\n");
    }
    if !refactors.is_empty() {
        out.push_str("### Performance & Refactoring\n\n");
        out.push_str(&refactors.join("\n"));
        out.push_str("\n\n");
    }
    if !others.is_empty() {
        out.push_str("### Maintenance & Chores\n\n");
        out.push_str(&others.join("\n"));
        out.push_str("\n\n");
    }
    out
}

pub fn run_release_sync(
    root: &Root,
    since_tag: Option<&str>,
    current_version: Option<&str>,
) -> Result<ReleaseSyncReport, ForgeError> {
    let tag = since_tag.unwrap_or("HEAD~10");
    let args = vec![
        "log".to_string(),
        format!("{}..HEAD", tag),
        "--format=%H %s".to_string(),
    ];

    let mut commits = Vec::new();
    if let Ok(out) = exec(root, Program::Git, &args, EXEC_DEADLINE) {
        for line in out.stdout.lines() {
            if let Some((hash, msg)) = line.split_once(' ') {
                commits.push(parse_conventional_commit(hash, msg));
            }
        }
    }

    let bump = determine_bump(&commits);
    let changelog_snippet = generate_changelog(&commits);

    let recommended_version = current_version.map(|v| {
        let parts: Vec<&str> = v.split('.').collect();
        if parts.len() == 3 {
            let major: u64 = parts[0].parse().unwrap_or(0);
            let minor: u64 = parts[1].parse().unwrap_or(0);
            let patch: u64 = parts[2].parse().unwrap_or(0);
            match bump {
                SemVerBump::Major => format!("{}.0.0", major + 1),
                SemVerBump::Minor => format!("{}.{}.0", major, minor + 1),
                SemVerBump::Patch => format!("{}.{}.{}", major, minor, patch + 1),
                SemVerBump::None => v.to_string(),
            }
        } else {
            v.to_string()
        }
    });

    let summary = format!(
        "Release Sync: Analyzed {} commit(s) since '{}'. Recommended bump: {:?}.",
        commits.len(),
        tag,
        bump
    );

    Ok(ReleaseSyncReport {
        since_tag: tag.to_string(),
        total_commits: commits.len(),
        recommended_bump: bump,
        recommended_version,
        changelog_snippet,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_and_bump_conventional_commits() {
        let c1 = parse_conventional_commit("abc1234", "feat(mcp): add blast_radius tool");
        assert_eq!(c1.commit_type, "feat");
        assert_eq!(c1.scope, Some("mcp".to_string()));

        let c2 = parse_conventional_commit("def5678", "fix: correct off-by-one in bisect");
        assert_eq!(c2.commit_type, "fix");

        let bump = determine_bump(&[c1, c2]);
        assert_eq!(bump, SemVerBump::Minor);
    }
}
