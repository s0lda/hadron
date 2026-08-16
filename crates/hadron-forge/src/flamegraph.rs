//! Pure logic for the `flamegraph` tool family.
//! Analyzes folded stack traces, identifies CPU hotspots, and generates flamegraph SVG visualizations.

use std::collections::BTreeMap;
use std::fs;
use serde::{Deserialize, Serialize};

use crate::file::{resolve_jailed_path, ForgeError, Root};

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum FlamegraphAction {
    AnalyzeFolded,
    TopHotspots,
    RenderSvg,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct HotspotFrame {
    pub name: String,
    pub self_samples: u64,
    pub total_samples: u64,
    pub self_percentage: f64,
    pub total_percentage: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct FlamegraphReport {
    pub total_samples: u64,
    pub total_stacks: usize,
    pub top_hotspots: Vec<HotspotFrame>,
    pub svg_path: Option<String>,
    pub summary: String,
}

/// Parse folded stack format lines: `frame1;frame2;frame3 count`
pub fn parse_folded_stacks(input: &str) -> (BTreeMap<Vec<String>, u64>, u64) {
    let mut stacks = BTreeMap::new();
    let mut total_samples = 0;

    for line in input.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }

        if let Some((stack_str, count_str)) = trimmed.rsplit_once(' ') {
            if let Ok(count) = count_str.parse::<u64>() {
                let frames: Vec<String> = stack_str
                    .split(';')
                    .map(|s| s.trim().to_string())
                    .filter(|s| !s.is_empty())
                    .collect();
                if !frames.is_empty() {
                    *stacks.entry(frames).or_insert(0) += count;
                    total_samples += count;
                }
            }
        }
    }

    (stacks, total_samples)
}

/// Calculate top hotspot frames from folded stacks.
pub fn calculate_hotspots(
    stacks: &BTreeMap<Vec<String>, u64>,
    total_samples: u64,
    top_limit: usize,
) -> Vec<HotspotFrame> {
    if total_samples == 0 {
        return Vec::new();
    }

    let mut self_counts: BTreeMap<String, u64> = BTreeMap::new();
    let mut total_counts: BTreeMap<String, u64> = BTreeMap::new();

    for (frames, count) in stacks {
        if let Some(leaf) = frames.last() {
            *self_counts.entry(leaf.clone()).or_insert(0) += count;
        }

        let mut seen_in_stack = std::collections::HashSet::new();
        for frame in frames {
            if seen_in_stack.insert(frame) {
                *total_counts.entry(frame.clone()).or_insert(0) += count;
            }
        }
    }

    let mut hotspots = Vec::new();
    for (name, total) in total_counts {
        let self_s = *self_counts.get(&name).unwrap_or(&0);
        let self_pct = (self_s as f64 / total_samples as f64) * 100.0;
        let total_pct = (total as f64 / total_samples as f64) * 100.0;

        hotspots.push(HotspotFrame {
            name,
            self_samples: self_s,
            total_samples: total,
            self_percentage: self_pct,
            total_percentage: total_pct,
        });
    }

    // Sort by self percentage descending, then total percentage
    hotspots.sort_by(|a, b| {
        b.self_percentage
            .partial_cmp(&a.self_percentage)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| b.total_percentage.partial_cmp(&a.total_percentage).unwrap_or(std::cmp::Ordering::Equal))
    });

    hotspots.truncate(top_limit);
    hotspots
}

/// Render lightweight standalone SVG flamegraph.
pub fn generate_flamegraph_svg(
    stacks: &BTreeMap<Vec<String>, u64>,
    total_samples: u64,
    title: &str,
) -> String {
    let width = 1200;
    let row_height = 20;
    let mut max_depth = 1;

    for frames in stacks.keys() {
        max_depth = max_depth.max(frames.len());
    }

    let height = (max_depth + 3) * row_height + 40;

    let mut svg = String::new();
    svg.push_str(&format!(
        "<svg version=\"1.1\" width=\"{}\" height=\"{}\" xmlns=\"http://www.w3.org/2000/svg\">\n",
        width, height
    ));
    svg.push_str("<style>\n");
    svg.push_str("  text { font-family: -apple-system, BlinkMacSystemFont, \"Segoe UI\", Roboto, sans-serif; font-size: 11px; fill: #111827; }\n");
    svg.push_str("  rect { stroke: #ffffff; stroke-width: 0.5; }\n");
    svg.push_str("  rect:hover { opacity: 0.8; cursor: pointer; }\n");
    svg.push_str("</style>\n");
    svg.push_str("<rect width=\"100%\" height=\"100%\" fill=\"#f9fafb\"/>\n");
    svg.push_str(&format!(
        "<text x=\"20\" y=\"25\" font-weight=\"bold\" font-size=\"14px\">{} (Total Samples: {})</text>\n",
        title, total_samples
    ));

    // Simple flame visualizer
    let colors = ["#f87171", "#fb923c", "#fbbf24", "#34d399", "#60a5fa", "#a78bfa", "#f472b6"];
    let mut color_idx = 0;

    let mut x_offset = 20.0;
    let usable_width = (width - 40) as f64;

    for (frames, count) in stacks {
        let stack_width = (usable_width * (*count as f64 / total_samples.max(1) as f64)).max(2.0);
        for (depth, frame) in frames.iter().enumerate() {
            let y = height as f64 - ((depth + 2) * row_height) as f64;
            let color = colors[color_idx % colors.len()];
            color_idx += 1;

            let label = if stack_width > 40.0 {
                let max_chars = (stack_width / 7.0) as usize;
                if frame.len() > max_chars && max_chars > 3 {
                    format!("{}…", &frame[..max_chars - 1])
                } else {
                    frame.clone()
                }
            } else {
                String::new()
            };

            svg.push_str(&format!(
                r#"<rect x="{x_offset:.1}" y="{y:.1}" width="{stack_width:.1}" height="{row_height}" fill="{color}"><title>{frame} ({count} samples)</title></rect>
<text x="{:.1}" y="{:.1}">{label}</text>
"#,
                x_offset + 3.0,
                y + 14.0
            ));
        }
        x_offset += stack_width;
    }

    svg.push_str("</svg>\n");
    svg
}

pub fn run_flamegraph(
    root: &Root,
    action: FlamegraphAction,
    folded_content: Option<&str>,
    folded_file: Option<&str>,
    output_svg_rel: Option<&str>,
    title: Option<&str>,
) -> Result<FlamegraphReport, ForgeError> {
    let content = match (folded_file, folded_content) {
        (Some(file), _) => {
            let abs_path = resolve_jailed_path(root, file)?;
            fs::read_to_string(&abs_path)
                .map_err(|e| ForgeError::Io(format!("Failed reading folded stack file {file}: {e}")))?
        }
        (None, Some(text)) => text.to_string(),
        (None, None) => {
            return Err(ForgeError::Rejected(
                "Either folded_file or folded_content must be provided to flamegraph".to_string(),
            ))
        }
    };

    let (stacks, total_samples) = parse_folded_stacks(&content);
    let top_hotspots = calculate_hotspots(&stacks, total_samples, 10);

    let mut svg_path = None;
    if action == FlamegraphAction::RenderSvg || output_svg_rel.is_some() {
        let svg_title = title.unwrap_or("CPU Flamegraph Profile");
        let svg = generate_flamegraph_svg(&stacks, total_samples, svg_title);
        let target_rel = output_svg_rel.unwrap_or(".hadron/screenshots/flamegraph.svg");
        let abs_svg = resolve_jailed_path(root, target_rel)?;
        if let Some(parent) = abs_svg.parent() {
            fs::create_dir_all(parent).map_err(|e| ForgeError::Io(e.to_string()))?;
        }
        fs::write(&abs_svg, svg).map_err(|e| ForgeError::Io(e.to_string()))?;
        svg_path = Some(target_rel.to_string());
    }

    let summary = format!(
        "Flamegraph Profile: {} total samples across {} unique stack traces. Top bottleneck: {}",
        total_samples,
        stacks.len(),
        top_hotspots.first().map(|h| format!("'{}' ({:.1}% self, {:.1}% total)", h.name, h.self_percentage, h.total_percentage)).unwrap_or_else(|| "None".to_string())
    );

    Ok(FlamegraphReport {
        total_samples,
        total_stacks: stacks.len(),
        top_hotspots,
        svg_path,
        summary,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_folded_and_calculate_hotspots() {
        let input = "main;render;rasterize 70\nmain;render;flush 30\nmain;idle 100\n";
        let (stacks, total) = parse_folded_stacks(input);
        assert_eq!(total, 200);
        assert_eq!(stacks.len(), 3);

        let hotspots = calculate_hotspots(&stacks, total, 5);
        assert_eq!(hotspots[0].name, "idle");
        assert_eq!(hotspots[0].self_samples, 100);
        assert_eq!(hotspots[0].self_percentage, 50.0);

        assert_eq!(hotspots[1].name, "rasterize");
        assert_eq!(hotspots[1].self_samples, 70);
        assert_eq!(hotspots[1].self_percentage, 35.0);
    }
}
