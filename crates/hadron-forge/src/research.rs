//! Autonomous Research Paper & Investigation Lifecycle.
//!
//! Provides structured research creation, listing, and inspection under `.hadron/docs/research/`.

use std::fs;
use std::path::{Path, PathBuf};
use serde::{Deserialize, Serialize};
use crate::file::{ForgeError, Root};

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchDocSummary {
    pub path: String,
    pub filename: String,
    pub title: String,
    pub size_bytes: u64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchWriteInput {
    pub slug: String,
    pub title: String,
    #[serde(default)]
    pub author: Option<String>,
    #[serde(default)]
    pub target_area: Option<String>,
    #[serde(default)]
    pub executive_summary: Option<String>,
    #[serde(default)]
    pub key_findings: Option<String>,
    #[serde(default)]
    pub constraints: Option<String>,
    #[serde(default)]
    pub trade_offs: Option<String>,
    #[serde(default)]
    pub recommendations: Option<String>,
    #[serde(default)]
    pub custom_body: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResearchWriteOutput {
    pub rel_path: String,
    pub full_path: String,
    pub title: String,
    pub bytes_written: usize,
}

pub fn research_dir_of(root: &Root) -> PathBuf {
    root.path().join(".hadron").join("docs").join("research")
}

fn current_date_str() -> String {
    let now = std::time::SystemTime::now();
    let since_epoch = now.duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_secs();
    let days = since_epoch / 86400;
    let mut year = 1970;
    let mut d = days;
    loop {
        let leap = (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0);
        let days_in_year = if leap { 366 } else { 365 };
        if d < days_in_year {
            let days_in_months = if leap {
                [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            } else {
                [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
            };
            let mut month = 1;
            for &m_days in &days_in_months {
                if d < m_days {
                    let day = d + 1;
                    return format!("{:04}-{:02}-{:02}", year, month, day);
                }
                d -= m_days;
                month += 1;
            }
            return format!("{:04}-01-01", year);
        }
        d -= days_in_year;
        year += 1;
    }
}

pub fn parse_research_title(content: &str) -> Option<String> {
    for line in content.lines() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("# Research:") {
            let t = rest.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        } else if let Some(rest) = trimmed.strip_prefix("# ") {
            let t = rest.trim();
            if !t.is_empty() {
                return Some(t.to_string());
            }
        }
    }
    None
}

pub fn format_research_document(input: &ResearchWriteInput, date_str: &str) -> String {
    if let Some(custom) = &input.custom_body {
        if !custom.trim().is_empty() {
            return custom.clone();
        }
    }

    let author = input.author.as_deref().unwrap_or("Autonomous Quark");
    let target = input.target_area.as_deref().unwrap_or("Workspace / Architecture");
    let summary = input.executive_summary.as_deref().unwrap_or("Exploratory investigation findings.");
    let findings = input.key_findings.as_deref().unwrap_or("Analysis of codebase mechanisms, APIs, and dependencies.");
    let constraints = input.constraints.as_deref().unwrap_or("Invariants, performance boundaries, and type safety constraints.");
    let tradeoffs = input.trade_offs.as_deref().unwrap_or("Evaluation of candidate approaches with trade-off analysis.");
    let recs = input.recommendations.as_deref().unwrap_or("Actionable next steps (e.g. author design spec and implementation plan).");

    format!(
        "# Research: {}\n\n\
        - **Date**: {}\n\
        - **Author**: {}\n\
        - **Status**: Completed\n\
        - **Target Area**: {}\n\n\
        ---\n\n\
        ## 1. Executive Summary\n\
        {}\n\n\
        ## 2. Key Findings & Current State Analysis\n\
        {}\n\n\
        ## 3. Technical Constraints & Invariants\n\
        {}\n\n\
        ## 4. Approaches & Trade-offs\n\
        {}\n\n\
        ## 5. Architectural Recommendations & Next Steps\n\
        {}\n",
        input.title.trim(),
        date_str,
        author,
        target,
        summary.trim(),
        findings.trim(),
        constraints.trim(),
        tradeoffs.trim(),
        recs.trim(),
    )
}

pub fn write_research(root: &Root, input: &ResearchWriteInput) -> Result<ResearchWriteOutput, ForgeError> {
    let clean_slug = input.slug.trim().trim_matches('/');
    if clean_slug.is_empty() {
        return Err(ForgeError::Rejected("slug cannot be empty".into()));
    }

    let date_str = current_date_str();
    let file_name = if clean_slug.ends_with(".md") {
        clean_slug.to_string()
    } else {
        format!("{date_str}-{clean_slug}-research.md")
    };

    let dir = research_dir_of(root);
    fs::create_dir_all(&dir).map_err(|e| ForgeError::Io(e.to_string()))?;

    let full_path = dir.join(&file_name);
    let rel_path = format!(".hadron/docs/research/{file_name}");

    let content = format_research_document(input, &date_str);
    fs::write(&full_path, &content).map_err(|e| ForgeError::Io(e.to_string()))?;

    Ok(ResearchWriteOutput {
        rel_path,
        full_path: full_path.display().to_string(),
        title: input.title.clone(),
        bytes_written: content.len(),
    })
}

pub fn list_research(root: &Root) -> Result<Vec<ResearchDocSummary>, ForgeError> {
    let dir = research_dir_of(root);
    if !dir.exists() {
        return Ok(Vec::new());
    }

    let mut results = Vec::new();
    let entries = fs::read_dir(&dir).map_err(|e| ForgeError::Io(e.to_string()))?;

    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() && path.extension().and_then(|e| e.to_str()) == Some("md") {
            let filename = path.file_name().and_then(|f| f.to_str()).unwrap_or_default().to_string();
            let rel_path = format!(".hadron/docs/research/{filename}");
            let metadata = entry.metadata().ok();
            let size_bytes = metadata.map(|m| m.len()).unwrap_or(0);
            let content = fs::read_to_string(&path).unwrap_or_default();
            let title = parse_research_title(&content).unwrap_or_else(|| filename.clone());

            results.push(ResearchDocSummary {
                path: rel_path,
                filename,
                title,
                size_bytes,
            });
        }
    }

    results.sort_by(|a, b| b.filename.cmp(&a.filename));
    Ok(results)
}

pub fn read_research(root: &Root, rel_or_abs_path: &str) -> Result<String, ForgeError> {
    let path = if Path::new(rel_or_abs_path).is_absolute() {
        PathBuf::from(rel_or_abs_path)
    } else {
        root.path().join(rel_or_abs_path.trim_start_matches('/'))
    };

    if !path.exists() {
        return Err(ForgeError::NotFound);
    }

    fs::read_to_string(&path).map_err(|e| ForgeError::Io(e.to_string()))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn write_and_read_and_list_research_round_trip() {
        let dir = tempdir().unwrap();
        let root = Root::new(dir.path());

        let input = ResearchWriteInput {
            slug: "dynamic-themes".to_string(),
            title: "Dynamic Theme Engine Architecture".to_string(),
            author: Some("@architect".to_string()),
            target_area: Some("crates/hadron-chamber".to_string()),
            executive_summary: Some("Complete custom theme system".to_string()),
            key_findings: Some("Colors should be configurable per token".to_string()),
            constraints: Some("Must maintain fast lockless access".to_string()),
            trade_offs: Some("ArcSwap vs RwLock".to_string()),
            recommendations: Some("Proceed to spec and plan".to_string()),
            custom_body: None,
        };

        let output = write_research(&root, &input).unwrap();
        assert!(output.rel_path.starts_with(".hadron/docs/research/"));
        assert!(output.rel_path.ends_with("-dynamic-themes-research.md"));

        let content = read_research(&root, &output.rel_path).unwrap();
        assert!(content.contains("# Research: Dynamic Theme Engine Architecture"));
        assert!(content.contains("- **Author**: @architect"));

        let list = list_research(&root).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].title, "Dynamic Theme Engine Architecture");
    }
}
