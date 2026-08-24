//! Pure logic for the `trace_slicer` tool family.
//! Compacts stack backtraces, compiler diagnostic cascades, and log streams into actionable root-cause summaries.

use serde::{Deserialize, Serialize};

use crate::file::ForgeError;

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum TraceSlicerAction {
    SliceBacktrace,
    CompactCompilerErrors,
    FilterLogSpans,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct SlicedTraceReport {
    pub original_lines: usize,
    pub sliced_lines: usize,
    pub compression_ratio_pct: f32,
    pub root_cause: Option<String>,
    pub relevant_frames: Vec<String>,
    pub formatted_output: String,
}

/// Slices a raw backtrace, keeping frames matching `project_crates` and collapsing stdlib/runtime frames.
pub fn slice_backtrace(raw: &str, project_crates: &[&str], max_lines: usize) -> SlicedTraceReport {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let original_lines = raw_lines.len();

    let mut root_cause = None;
    let mut frames = Vec::new();
    let mut relevant_frames = Vec::new();
    let mut collapsed_count = 0;

    for line in &raw_lines {
        let trimmed = line.trim();
        if trimmed.starts_with("thread '") && trimmed.contains("panicked at") {
            root_cause = Some(trimmed.to_string());
            continue;
        }
        if trimmed.starts_with("panicked at") {
            root_cause = Some(trimmed.to_string());
            continue;
        }

        // Frame detection: e.g. "0: ...", "1: ...", "at /path/..."
        let is_frame = trimmed.chars().next().map_or(false, |c| c.is_ascii_digit())
            && trimmed.contains(':');

        if is_frame {
            let is_relevant = if project_crates.is_empty() {
                !trimmed.contains("core::")
                    && !trimmed.contains("std::")
                    && !trimmed.contains("alloc::")
                    && !trimmed.contains("rust_begin_unwind")
            } else {
                project_crates.iter().any(|c| trimmed.contains(c))
            };

            if is_relevant {
                if collapsed_count > 0 {
                    frames.push(format!("  [... {} external/runtime frames ...]", collapsed_count));
                    collapsed_count = 0;
                }
                frames.push(format!("  {}", trimmed));
                relevant_frames.push(trimmed.to_string());
            } else {
                collapsed_count += 1;
            }
        } else if trimmed.starts_with("at ") && !relevant_frames.is_empty() {
            // Source location line following a frame
            if collapsed_count == 0 {
                frames.push(format!("    {}", trimmed));
            }
        } else if trimmed.starts_with("stack backtrace:") {
            // Skip header or format cleanly
        }
    }

    if collapsed_count > 0 {
        frames.push(format!("  [... {} external/runtime frames ...]", collapsed_count));
    }

    let mut output_lines = Vec::new();
    if let Some(ref rc) = root_cause {
        output_lines.push(format!("ROOT CAUSE: {}", rc));
        output_lines.push("STACK TRACE (SLICED):".to_string());
    } else {
        output_lines.push("STACK TRACE (SLICED):".to_string());
    }

    for f in frames {
        if output_lines.len() >= max_lines {
            output_lines.push("  [... truncated by line limit ...]".to_string());
            break;
        }
        output_lines.push(f);
    }

    let formatted_output = output_lines.join("\n");
    let sliced_lines = output_lines.len();
    let compression_ratio_pct = if original_lines > 0 {
        ((original_lines.saturating_sub(sliced_lines)) as f32 / original_lines as f32) * 100.0
    } else {
        0.0
    };

    SlicedTraceReport {
        original_lines,
        sliced_lines,
        compression_ratio_pct,
        root_cause,
        relevant_frames,
        formatted_output,
    }
}

/// Compacts compiler error cascades, extracting error codes, primary locations, and deduplicating notes.
pub fn compact_compiler_errors(raw: &str, max_lines: usize) -> SlicedTraceReport {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let original_lines = raw_lines.len();

    let mut output_lines = Vec::new();
    let mut relevant_frames = Vec::new();
    let mut root_cause = None;
    let mut in_primary_error = false;

    for line in &raw_lines {
        let trimmed = line.trim();
        if trimmed.starts_with("error[E") || (trimmed.starts_with("error:") && !trimmed.contains("could not compile")) {
            if root_cause.is_none() {
                root_cause = Some(trimmed.to_string());
            }
            relevant_frames.push(trimmed.to_string());
            output_lines.push(trimmed.to_string());
            in_primary_error = true;
        } else if trimmed.starts_with("--> ") {
            output_lines.push(format!("  {}", trimmed));
            in_primary_error = true;
        } else if trimmed.starts_with("warning:") || trimmed.starts_with("warning[") {
            // Keep top level warnings compact
            if output_lines.len() < max_lines / 2 {
                output_lines.push(trimmed.to_string());
            }
            in_primary_error = false;
        } else if in_primary_error && (trimmed.starts_with("|") || trimmed.contains("^^^")) {
            // Code snippet highlight
            output_lines.push(format!("  {}", trimmed));
        } else if trimmed.starts_with("help: ") {
            output_lines.push(format!("  {}", trimmed));
            in_primary_error = false;
        } else if trimmed.is_empty() {
            in_primary_error = false;
        }

        if output_lines.len() >= max_lines {
            output_lines.push("[... additional errors truncated ...]".to_string());
            break;
        }
    }

    let formatted_output = if output_lines.is_empty() {
        "No compiler errors or warnings detected in raw output.".to_string()
    } else {
        output_lines.join("\n")
    };

    let sliced_lines = output_lines.len();
    let compression_ratio_pct = if original_lines > 0 {
        ((original_lines.saturating_sub(sliced_lines)) as f32 / original_lines as f32) * 100.0
    } else {
        0.0
    };

    SlicedTraceReport {
        original_lines,
        sliced_lines,
        compression_ratio_pct,
        root_cause,
        relevant_frames,
        formatted_output,
    }
}

/// Filters log lines by min level and optional search term.
pub fn filter_log_spans(
    raw: &str,
    min_level: &str,
    filter_term: Option<&str>,
    max_lines: usize,
) -> SlicedTraceReport {
    let raw_lines: Vec<&str> = raw.lines().collect();
    let original_lines = raw_lines.len();

    let level_rank = |l: &str| -> u8 {
        if l.contains("ERROR") || l.contains("error") {
            3
        } else if l.contains("WARN") || l.contains("warn") {
            2
        } else if l.contains("INFO") || l.contains("info") {
            1
        } else {
            0
        }
    };

    let target_rank = level_rank(min_level);

    let mut output_lines = Vec::new();
    let mut relevant_frames = Vec::new();
    let mut root_cause = None;

    for line in &raw_lines {
        let trimmed = line.trim();
        if trimmed.is_empty() {
            continue;
        }

        let l_rank = if trimmed.contains("ERROR") || trimmed.contains("[ERROR]") {
            3
        } else if trimmed.contains("WARN") || trimmed.contains("[WARN]") {
            2
        } else if trimmed.contains("INFO") || trimmed.contains("[INFO]") {
            1
        } else {
            0
        };

        if l_rank >= target_rank {
            if let Some(term) = filter_term {
                if !trimmed.to_lowercase().contains(&term.to_lowercase()) {
                    continue;
                }
            }

            if l_rank == 3 && root_cause.is_none() {
                root_cause = Some(trimmed.to_string());
            }

            relevant_frames.push(trimmed.to_string());
            output_lines.push(trimmed.to_string());

            if output_lines.len() >= max_lines {
                output_lines.push("[... logs truncated by line limit ...]".to_string());
                break;
            }
        }
    }

    let formatted_output = if output_lines.is_empty() {
        "No matching log spans found.".to_string()
    } else {
        output_lines.join("\n")
    };

    let sliced_lines = output_lines.len();
    let compression_ratio_pct = if original_lines > 0 {
        ((original_lines.saturating_sub(sliced_lines)) as f32 / original_lines as f32) * 100.0
    } else {
        0.0
    };

    SlicedTraceReport {
        original_lines,
        sliced_lines,
        compression_ratio_pct,
        root_cause,
        relevant_frames,
        formatted_output,
    }
}

pub fn run_trace_slicer(
    action: TraceSlicerAction,
    raw_text: &str,
    project_crates: Option<&[&str]>,
    max_lines: Option<usize>,
    filter_term: Option<&str>,
    min_level: Option<&str>,
) -> Result<SlicedTraceReport, ForgeError> {
    let limit = max_lines.unwrap_or(50);
    let crates = project_crates.unwrap_or(&[]);

    match action {
        TraceSlicerAction::SliceBacktrace => Ok(slice_backtrace(raw_text, crates, limit)),
        TraceSlicerAction::CompactCompilerErrors => Ok(compact_compiler_errors(raw_text, limit)),
        TraceSlicerAction::FilterLogSpans => Ok(filter_log_spans(
            raw_text,
            min_level.unwrap_or("INFO"),
            filter_term,
            limit,
        )),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_backtrace_slicing_collapses_stdlib_frames() {
        let raw = r#"
thread 'main' panicked at 'index out of bounds', crates/hadron-chamber/src/render.rs:42:5
stack backtrace:
   0: rust_begin_unwind
   1: core::panicking::panic_fmt
   2: hadron_chamber::app::render::chat::render_message
      at /home/Jake/dev/hadron/crates/hadron-chamber/src/app/render/chat.rs:42
   3: <hadron_chamber::app::App as gpui::View>::render
   4: gpui::window::Window::draw
   5: std::sys_common::backtrace::__rust_begin_short_backtrace
        "#;
        let report = slice_backtrace(raw, &["hadron_chamber"], 10);
        assert!(report.compression_ratio_pct > 0.0);
        assert_eq!(report.root_cause, Some("thread 'main' panicked at 'index out of bounds', crates/hadron-chamber/src/render.rs:42:5".to_string()));
        assert!(report.formatted_output.contains("hadron_chamber::app::render::chat::render_message"));
        assert!(report.formatted_output.contains("[... 2 external/runtime frames ...]"));
    }

    #[test]
    fn test_compact_compiler_errors() {
        let raw = r#"
error[E0308]: mismatched types
  --> src/main.rs:10:9
   |
10 |     let x: u32 = "hello";
   |            ---   ^^^^^^^ expected `u32`, found `&str`
   |            |
   |            expected due to this
note: some noisy note 1
note: some noisy note 2
note: some noisy note 3
error[E0425]: cannot find value `y` in this scope
  --> src/main.rs:12:5
        "#;
        let report = compact_compiler_errors(raw, 20);
        assert!(report.compression_ratio_pct > 0.0);
        assert_eq!(report.relevant_frames.len(), 2);
        assert!(report.formatted_output.contains("error[E0308]"));
        assert!(report.formatted_output.contains("error[E0425]"));
    }

    #[test]
    fn test_filter_log_spans() {
        let raw = r#"
2026-08-24T12:00:00 [DEBUG] heart beat ok
2026-08-24T12:00:01 [INFO] session initialized
2026-08-24T12:00:02 [WARN] high memory pressure
2026-08-24T12:00:03 [ERROR] failed to connect to database
        "#;
        let report = filter_log_spans(raw, "WARN", None, 10);
        assert_eq!(report.relevant_frames.len(), 2);
        assert!(report.formatted_output.contains("[WARN]"));
        assert!(report.formatted_output.contains("[ERROR]"));
        assert!(!report.formatted_output.contains("[INFO]"));
    }
}
