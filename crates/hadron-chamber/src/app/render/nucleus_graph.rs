//! 2D Force-Directed Nucleus Knowledge Graph.
//!
//! Provides a physical spring-embedder force simulation calculating dynamic 2D coordinates
//! for nucleus notes, invariants, and features connected by wiki-links, with Lavapipe caching
//! and interactive node selection.

use std::collections::{BTreeMap, HashMap, HashSet};
use serde::{Deserialize, Serialize};
use super::*;
use gpui::{canvas, fill, point, px, size, Bounds, MouseButton, PathBuilder};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForceNode {
    pub slug: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForceEdge {
    pub source: String,
    pub target: String,
}

#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForceDirectedGraph {
    pub nodes: HashMap<String, ForceNode>,
    pub edges: Vec<ForceEdge>,
}

impl ForceDirectedGraph {
    pub fn new() -> Self {
        Self {
            nodes: HashMap::new(),
            edges: Vec::new(),
        }
    }

    pub fn add_node(&mut self, slug: impl Into<String>, x: f32, y: f32) {
        let s = slug.into();
        self.nodes.insert(
            s.clone(),
            ForceNode {
                slug: s,
                x,
                y,
                vx: 0.0,
                vy: 0.0,
            },
        );
    }

    pub fn add_edge(&mut self, source: impl Into<String>, target: impl Into<String>) {
        self.edges.push(ForceEdge {
            source: source.into(),
            target: target.into(),
        });
    }

    /// Advances the spring-embedder physical simulation by one tick.
    pub fn step_simulation(&mut self, repulsion_k: f32, attraction_k: f32, damping: f32) {
        let slugs: Vec<String> = self.nodes.keys().cloned().collect();

        // 1. Repulsive forces between all node pairs
        for i in 0..slugs.len() {
            for j in (i + 1)..slugs.len() {
                let n1 = &self.nodes[&slugs[i]];
                let n2 = &self.nodes[&slugs[j]];

                let dx = n2.x - n1.x;
                let dy = n2.y - n1.y;
                let dist_sq = (dx * dx + dy * dy).max(1.0);
                let dist = dist_sq.sqrt();

                let force = repulsion_k / dist_sq;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                if let Some(node1) = self.nodes.get_mut(&slugs[i]) {
                    node1.vx -= fx;
                    node1.vy -= fy;
                }
                if let Some(node2) = self.nodes.get_mut(&slugs[j]) {
                    node2.vx += fx;
                    node2.vy += fy;
                }
            }
        }

        // 2. Attractive forces along edges (Hooke's law with ideal rest length so linked nodes don't collapse to 0)
        const IDEAL_EDGE_LEN: f32 = 55.0;
        for edge in &self.edges {
            if let (Some(n1), Some(n2)) = (self.nodes.get(&edge.source), self.nodes.get(&edge.target)) {
                let dx = n2.x - n1.x;
                let dy = n2.y - n1.y;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);

                let force = (dist - IDEAL_EDGE_LEN).max(0.0) * attraction_k;
                let fx = (dx / dist) * force;
                let fy = (dy / dist) * force;

                if let Some(node1) = self.nodes.get_mut(&edge.source) {
                    node1.vx += fx;
                    node1.vy += fy;
                }
                if let Some(node2) = self.nodes.get_mut(&edge.target) {
                    node2.vx -= fx;
                    node2.vy -= fy;
                }
            }
        }

        // 3. Centering gravity toward (400.0, 300.0) so orphan/disconnected nodes stay compact
        //    and Apply velocity + damping
        const CENTER_X: f32 = 400.0;
        const CENTER_Y: f32 = 300.0;
        const GRAVITY_K: f32 = 0.035;
        for node in self.nodes.values_mut() {
            node.vx += (CENTER_X - node.x) * GRAVITY_K;
            node.vy += (CENTER_Y - node.y) * GRAVITY_K;
            node.x += node.vx;
            node.y += node.vy;
            node.vx *= damping;
            node.vy *= damping;
        }
    }
}

/// Lavapipe-cached Nucleus Graph topology and layout calculation.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CachedNucleusGraph {
    pub nodes: BTreeMap<String, hadron_forge::nucleus_graph::NucleusNode>,
    pub dead_links: Vec<(String, String)>,
    pub orphaned_notes: Vec<String>,
    pub force_graph: ForceDirectedGraph,
    pub positions: HashMap<String, (f32, f32)>,
}

/// Replaces broken wiki-links (`[[target_slug]]`) and markdown links (`[label](notes/target_slug.md)`)
/// to `target_slug` with inline code or plain label text so the note no longer carries a dead link.
pub fn remove_dead_link_from_markdown(content: &str, target_slug: &str) -> String {
    let wiki_pattern = format!("[[{}]]", target_slug);
    let replacement_code = format!("`{}`", target_slug);
    let mut result = content.replace(&wiki_pattern, &replacement_code);

    for suffix in [".md", ""] {
        let needle = format!("](notes/{}{})", target_slug, suffix);
        while let Some(mid_ix) = result.find(&needle) {
            let after_ix = mid_ix + needle.len();
            if let Some(open_bracket_rel) = result[..mid_ix].rfind('[') {
                let label = result[open_bracket_rel + 1..mid_ix].to_string();
                let replacement = if label.is_empty() {
                    replacement_code.clone()
                } else {
                    label
                };
                result.replace_range(open_bracket_rel..after_ix, &replacement);
            } else {
                break;
            }
        }
    }

    result
}

/// Removes a broken outgoing link to `target_slug` from `notes/<source_slug>.md`.
pub fn remove_dead_link_from_note(
    nucleus_dir: &std::path::Path,
    source_slug: &str,
    target_slug: &str,
) -> std::io::Result<()> {
    let note_path = nucleus_dir.join("notes").join(format!("{}.md", source_slug));
    let content = std::fs::read_to_string(&note_path)?;
    let updated = remove_dead_link_from_markdown(&content, target_slug);
    if updated != content {
        std::fs::write(&note_path, updated)?;
    }
    Ok(())
}

/// Extracts the `description:` frontmatter value from a note file, falling back to `slug`.
fn extract_note_description(content: &str, slug: &str) -> String {
    for line in content.lines().take(12) {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix("description:") {
            let desc = rest.trim().trim_matches('"').trim_matches('\'').trim();
            if !desc.is_empty() {
                return desc.chars().take(100).collect();
            }
        }
    }
    slug.to_string()
}

/// Adds a pointer line `- [<slug>](notes/<slug>.md) — <description>` to `index.md` if not present.
pub fn index_orphan_note(nucleus_dir: &std::path::Path, slug: &str) -> std::io::Result<()> {
    let note_path = nucleus_dir.join("notes").join(format!("{}.md", slug));
    let note_content = std::fs::read_to_string(&note_path).unwrap_or_default();
    let hook = extract_note_description(&note_content, slug);

    let index_path = nucleus_dir.join("index.md");
    let existing = std::fs::read_to_string(&index_path).unwrap_or_default();
    let target_marker = format!("(notes/{}.md)", slug);
    let wiki_marker = format!("[[{}]]", slug);
    if existing.contains(&target_marker) || existing.contains(&wiki_marker) {
        return Ok(());
    }

    let mut updated = existing;
    if !updated.is_empty() && !updated.ends_with('\n') {
        updated.push('\n');
    }
    updated.push_str(&format!("- [{}](notes/{}.md) — {}\n", slug, slug, hook));
    std::fs::write(&index_path, updated)
}

/// Deletes an orphaned note file `notes/<slug>.md`.
pub fn delete_orphan_note(nucleus_dir: &std::path::Path, slug: &str) -> std::io::Result<()> {
    let note_path = nucleus_dir.join("notes").join(format!("{}.md", slug));
    if note_path.exists() {
        std::fs::remove_file(note_path)?;
    }
    Ok(())
}

/// Creates a stub note `notes/<slug>.md` with standard YAML frontmatter and indexes it in `index.md`.
pub fn create_stub_note(nucleus_dir: &std::path::Path, slug: &str) -> std::io::Result<()> {
    let notes_dir = nucleus_dir.join("notes");
    std::fs::create_dir_all(&notes_dir)?;
    let note_path = notes_dir.join(format!("{}.md", slug));
    if !note_path.exists() {
        let stub = format!(
            "---\nname: {slug}\ndescription: {slug}\nmetadata:\n  type: project\n---\n\nStub note created from Nucleus Graph for `{slug}`.\n"
        );
        std::fs::write(&note_path, stub)?;
    }
    index_orphan_note(nucleus_dir, slug)
}

impl CachedNucleusGraph {
    pub fn build_and_save(repo_root: &std::path::Path) -> Self {
        let built = Self::build(repo_root);
        if repo_root.join(".hadron").is_dir() {
            let fp = super::visualizer::compute_nucleus_graph_fingerprint(repo_root);
            let mut disk_cache = super::visualizer::VisualizerGraphsDiskCache::load(repo_root);
            disk_cache.nucleus_fingerprint = Some(fp);
            disk_cache.nucleus_graph = Some(built.clone());
            let _ = disk_cache.save(repo_root);
        }
        built
    }

    pub fn build(repo_root: &std::path::Path) -> Self {
        let nucleus_dir = repo_root.join(".hadron/nucleus");
        let (nodes, dead_links) = hadron_forge::nucleus_graph::build_nucleus_graph(&nucleus_dir);

        let mut force_graph = ForceDirectedGraph::new();
        let mut orphaned_notes = Vec::new();

        // Seed nodes on a Vogel golden-angle sunflower spiral so 200+ notes start evenly
        // distributed across the 2D disc rather than crowded onto a 1D perimeter ring.
        const GOLDEN_ANGLE: f32 = 2.399_963_1;
        for (i, (slug, node)) in nodes.iter().enumerate() {
            let r = 22.0 * ((i + 1) as f32).sqrt();
            let angle = (i as f32) * GOLDEN_ANGLE;
            let init_x = 400.0 + r * angle.cos();
            let init_y = 300.0 + r * angle.sin();
            force_graph.add_node(slug, init_x, init_y);

            if node.incoming_links.is_empty() && node.outgoing_links.is_empty() {
                orphaned_notes.push(slug.clone());
            }

            for target in &node.outgoing_links {
                force_graph.add_edge(slug, target);
            }
        }

        // Run force simulation ticks to reach layout equilibrium
        for _ in 0..140 {
            force_graph.step_simulation(5500.0, 0.05, 0.85);
        }

        let mut positions = HashMap::new();
        for (slug, fn_node) in &force_graph.nodes {
            positions.insert(slug.clone(), (fn_node.x, fn_node.y));
        }

        Self {
            nodes,
            dead_links,
            orphaned_notes,
            force_graph,
            positions,
        }
    }
}

/// Projects 2D simulation coordinates into `(origin_x, origin_y, w, h)` screen bounds,
/// scaling X and Y independently with padding so large graphs fill wide or tall viewports cleanly.
pub(crate) fn compute_nucleus_projection(
    positions_map: &HashMap<String, (f32, f32)>,
    origin_x: f32,
    origin_y: f32,
    w: f32,
    h: f32,
) -> (f32, f32, f32, f32, f32, f32) {
    let cx_pt = origin_x + w / 2.0;
    let cy_pt = origin_y + h / 2.0;

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for &(x, y) in positions_map.values() {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    if min_x > max_x {
        min_x = 0.0;
        max_x = 100.0;
        min_y = 0.0;
        max_y = 100.0;
    }

    let range_x = (max_x - min_x).max(60.0);
    let range_y = (max_y - min_y).max(60.0);

    let avail_w = (w - 56.0).max(40.0);
    let avail_h = (h - 56.0).max(40.0);
    let scale_x = (avail_w / range_x).min(2.5);
    let scale_y = (avail_h / range_y).min(2.5);

    let center_orig_x = (min_x + max_x) * 0.5;
    let center_orig_y = (min_y + max_y) * 0.5;

    (cx_pt, cy_pt, center_orig_x, center_orig_y, scale_x, scale_y)
}

pub const NUCLEUS_ZOOM_MIN: f32 = 0.5;
pub const NUCLEUS_ZOOM_MAX: f32 = 8.0;

/// Projects 2D simulation coordinates into screen bounds with interactive `zoom` and `(pan_x, pan_y)` offset.
pub(crate) fn compute_nucleus_projection_with_view(
    positions_map: &HashMap<String, (f32, f32)>,
    origin_x: f32,
    origin_y: f32,
    w: f32,
    h: f32,
    zoom: f32,
    pan_x: f32,
    pan_y: f32,
) -> (f32, f32, f32, f32, f32, f32) {
    let (cx_pt, cy_pt, center_orig_x, center_orig_y, base_scale_x, base_scale_y) =
        compute_nucleus_projection(positions_map, origin_x, origin_y, w, h);
    let clamped_zoom = zoom.clamp(NUCLEUS_ZOOM_MIN, NUCLEUS_ZOOM_MAX);
    (
        cx_pt + pan_x,
        cy_pt + pan_y,
        center_orig_x,
        center_orig_y,
        base_scale_x * clamped_zoom,
        base_scale_y * clamped_zoom,
    )
}

/// Applies a zoom delta anchored at `cursor_pos` relative to `canvas_center` so the graph point
/// directly under the mouse cursor stays stationary on screen while zooming in or out.
pub(crate) fn apply_nucleus_zoom_at_cursor(
    current_zoom: f32,
    current_pan: (f32, f32),
    zoom_delta: f32,
    cursor_pos: (f32, f32),
    canvas_center: (f32, f32),
) -> (f32, (f32, f32)) {
    let old_zoom = current_zoom.clamp(NUCLEUS_ZOOM_MIN, NUCLEUS_ZOOM_MAX);
    let factor = (1.0 + zoom_delta).clamp(0.5, 2.0);
    let new_zoom = (old_zoom * factor).clamp(NUCLEUS_ZOOM_MIN, NUCLEUS_ZOOM_MAX);
    if (new_zoom - old_zoom).abs() < f32::EPSILON {
        return (new_zoom, current_pan);
    }
    let ratio = new_zoom / old_zoom;
    let mx = cursor_pos.0 - canvas_center.0;
    let my = cursor_pos.1 - canvas_center.1;
    let new_pan_x = mx - (mx - current_pan.0) * ratio;
    let new_pan_y = my - (my - current_pan.1) * ratio;
    (new_zoom, (new_pan_x, new_pan_y))
}

/// Filters Nucleus Graph nodes by active filter mode (`None`/`"all"`, `"orphans"`, `"dead_links"`, or search query).
pub(crate) fn filter_nucleus_nodes(
    cached: &CachedNucleusGraph,
    filter: Option<&str>,
) -> Vec<hadron_forge::nucleus_graph::NucleusNode> {
    let filter_mode = filter.unwrap_or("all");
    let is_orphan_filter = filter_mode == "orphans";
    let is_dead_link_filter = filter_mode == "dead_links";
    let filter_str = filter.unwrap_or("").trim().to_lowercase();
    let dead_sources: HashSet<&str> = cached.dead_links.iter().map(|(s, _)| s.as_str()).collect();

    cached
        .nodes
        .values()
        .filter(|n| {
            if is_orphan_filter && !cached.orphaned_notes.contains(&n.slug) {
                return false;
            }
            if is_dead_link_filter && !dead_sources.contains(n.slug.as_str()) {
                return false;
            }
            if filter_str.is_empty() || filter_str == "orphans" || filter_str == "dead_links" {
                true
            } else {
                n.slug.to_lowercase().contains(&filter_str) || n.title.to_lowercase().contains(&filter_str)
            }
        })
        .cloned()
        .collect()
}

impl super::Chamber {
    pub(super) fn nucleus_graph_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();

        if self.cached_nucleus_graph.borrow().is_none() {
            self.warm_visualizer_graphs(cx);
        }

        let cache_ref = self.cached_nucleus_graph.borrow();
        let mode_switcher = self.visualizer_mode_switcher_header(cx);

        if cache_ref.is_none() {
            return v_flex()
                .id("nucleus-graph-container")
                .size_full()
                .bg(theme::canvas_base())
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .px_4()
                        .py_2()
                        .border_b_1()
                        .border_color(theme::glass_highlight())
                        .child(mode_switcher),
                )
                .child(
                    v_flex()
                        .flex_1()
                        .items_center()
                        .justify_center()
                        .gap_2()
                        .child(
                            div()
                                .text_sm()
                                .text_color(theme::text_muted())
                                .child("Simulating Nucleus knowledge graph layout..."),
                        ),
                )
                .into_any_element();
        }

        let cached = cache_ref.as_ref().unwrap();

        let selected_note = self.visualizer_selected_note.clone();
        let hovered_note = self.nucleus_graph_hovered_note.clone();
        let filter_mode = self.visualizer_nucleus_filter.as_deref().unwrap_or("all");
        let is_orphan_filter = filter_mode == "orphans";
        let is_dead_link_filter = filter_mode == "dead_links";
        let is_all_filter = !is_orphan_filter && !is_dead_link_filter;
        let dead_sources: HashSet<String> = cached.dead_links.iter().map(|(s, _)| s.clone()).collect();
        let zoom = self.nucleus_graph_zoom.clamp(NUCLEUS_ZOOM_MIN, NUCLEUS_ZOOM_MAX);
        let (pan_x, pan_y) = self.nucleus_graph_pan;

        // Zoom Controls + Header Stats Badges
        let zoom_controls = h_flex()
            .gap_1()
            .items_center()
            .child(
                div()
                    .id("nucleus-zoom-out")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_muted())
                    .hover(|s| s.text_color(theme::text()))
                    .child("−")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let center = this.nucleus_graph_canvas_center.get();
                        let (nz, np) = apply_nucleus_zoom_at_cursor(
                            this.nucleus_graph_zoom,
                            this.nucleus_graph_pan,
                            -0.20,
                            center,
                            center,
                        );
                        this.nucleus_graph_zoom = nz;
                        this.nucleus_graph_pan = np;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .px_1p5()
                    .py_0p5()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("{:.0}%", zoom * 100.0)),
            )
            .child(
                div()
                    .id("nucleus-zoom-in")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_muted())
                    .hover(|s| s.text_color(theme::text()))
                    .child("+")
                    .on_click(cx.listener(|this, _, _, cx| {
                        let center = this.nucleus_graph_canvas_center.get();
                        let (nz, np) = apply_nucleus_zoom_at_cursor(
                            this.nucleus_graph_zoom,
                            this.nucleus_graph_pan,
                            0.25,
                            center,
                            center,
                        );
                        this.nucleus_graph_zoom = nz;
                        this.nucleus_graph_pan = np;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("nucleus-zoom-reset")
                    .px_2()
                    .py_0p5()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_muted())
                    .hover(|s| s.text_color(theme::text()))
                    .child("Reset")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.nucleus_graph_zoom = 1.0;
                        this.nucleus_graph_pan = (0.0, 0.0);
                        cx.notify();
                    })),
            );

        let is_revalidating = self.cached_nucleus_graph_loading.get();

        let stats_hud = h_flex()
            .gap_2()
            .flex_wrap()
            .items_center()
            .child(zoom_controls)
            .when(is_revalidating, |this| {
                this.child(
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(theme::tab_bar_bg())
                        .border_1()
                        .border_color(theme::glass_highlight())
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child("Checking changes…"),
                )
            })
            .child(
                div()
                    .px_2p5()
                    .py_1()
                    .rounded_full()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::accent())
                    .child(format!("{} Notes", cached.nodes.len())),
            )
            .child(
                div()
                    .px_2p5()
                    .py_1()
                    .rounded_full()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_secondary())
                    .child(format!("{} Links", cached.force_graph.edges.len())),
            )
            .when(!cached.orphaned_notes.is_empty(), |this| {
                this.child(
                    div()
                        .id("nucleus-hud-orphans")
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .cursor_pointer()
                        .bg(theme::warning().opacity(0.18))
                        .border_1()
                        .border_color(theme::warning().opacity(0.4))
                        .text_xs()
                        .text_color(theme::warning())
                        .child(format!("{} Orphans", cached.orphaned_notes.len()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.visualizer_nucleus_filter = Some("orphans".to_string());
                            cx.notify();
                        })),
                )
            })
            .when(!cached.dead_links.is_empty(), |this| {
                this.child(
                    div()
                        .id("nucleus-hud-dead-links")
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .cursor_pointer()
                        .bg(theme::error().opacity(0.18))
                        .border_1()
                        .border_color(theme::error().opacity(0.4))
                        .text_xs()
                        .text_color(theme::error())
                        .child(format!("{} Dead Links", cached.dead_links.len()))
                        .on_click(cx.listener(|this, _, _, cx| {
                            this.visualizer_nucleus_filter = Some("dead_links".to_string());
                            cx.notify();
                        })),
                )
            });

        let header = h_flex()
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .flex_wrap()
            .gap_2()
            .justify_between()
            .items_center()
            .px_4()
            .py_2()
            .border_b_1()
            .border_color(theme::glass_highlight())
            .child(mode_switcher)
            .child(stats_hud);

        // Searchable Note Inspector Dock with Filter Pills
        let filtered_notes = filter_nucleus_nodes(cached, self.visualizer_nucleus_filter.as_deref());
        let visible_node_slugs: HashSet<String> = filtered_notes.iter().map(|n| n.slug.clone()).collect();

        // 2D Node-Link Canvas Rendering
        let nodes_map: BTreeMap<String, hadron_forge::nucleus_graph::NucleusNode> = cached
            .nodes
            .iter()
            .filter(|(slug, _)| visible_node_slugs.contains(*slug))
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        let edges_list: Vec<ForceEdge> = cached
            .force_graph
            .edges
            .iter()
            .filter(|e| visible_node_slugs.contains(&e.source) && visible_node_slugs.contains(&e.target))
            .cloned()
            .collect();
        let positions_map = cached.positions.clone();
        let orphaned_set: HashSet<String> = cached.orphaned_notes.iter().cloned().collect();
        let dead_sources_set = dead_sources.clone();
        let dead_targets: HashSet<String> = cached.dead_links.iter().map(|(_, t)| t.clone()).collect();
        let sel_slug = selected_note.clone();
        let hov_slug = hovered_note.clone();

        let hit_positions_cell = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Bounds<Pixels>, f32, f32)>::new()));
        let hit_positions_for_paint = hit_positions_cell.clone();
        let canvas_center_cell = std::rc::Rc::new(std::cell::Cell::new(self.nucleus_graph_canvas_center.get()));
        let canvas_center_for_paint = canvas_center_cell.clone();

        let canvas_elem = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _cx| {
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                let origin_x = f32::from(bounds.origin.x);
                let origin_y = f32::from(bounds.origin.y);

                canvas_center_for_paint.set((origin_x + w * 0.5, origin_y + h * 0.5));

                let (cx_pt, cy_pt, center_orig_x, center_orig_y, scale_x, scale_y) =
                    compute_nucleus_projection_with_view(
                        &positions_map,
                        origin_x,
                        origin_y,
                        w,
                        h,
                        zoom,
                        pan_x,
                        pan_y,
                    );

                let project = |x: f32, y: f32| -> (f32, f32) {
                    let sx = cx_pt + (x - center_orig_x) * scale_x;
                    let sy = cy_pt + (y - center_orig_y) * scale_y;
                    (sx, sy)
                };

                let mut painted_hits = Vec::new();

                // 1. Edges
                for edge in &edges_list {
                    if let (Some(&(x1, y1)), Some(&(x2, y2))) = (positions_map.get(&edge.source), positions_map.get(&edge.target)) {
                        let (sx1, sy1) = project(x1, y1);
                        let (sx2, sy2) = project(x2, y2);

                        let mut builder = PathBuilder::stroke(px(1.2));
                        builder.move_to(point(px(sx1), px(sy1)));
                        builder.line_to(point(px(sx2), px(sy2)));

                        if let Ok(path) = builder.build() {
                            let is_dead = dead_targets.contains(&edge.target);
                            let color = if is_dead {
                                gpui::rgb(0xf38ba8).opacity(0.55)
                            } else {
                                gpui::rgb(0xa87ffb).opacity(0.35)
                            };
                            window.paint_path(path, color);
                        }
                    }
                }

                // 2. Nodes
                for (slug, _node) in &nodes_map {
                    if let Some(&(x, y)) = positions_map.get(slug) {
                        let (sx, sy) = project(x, y);
                        let center = point(px(sx), px(sy));

                        let is_selected = sel_slug.as_deref() == Some(slug.as_str());
                        let is_hovered = hov_slug.as_deref() == Some(slug.as_str());
                        let is_orphan = orphaned_set.contains(slug);
                        let has_dead_link = dead_sources_set.contains(slug);

                        let radius = px(if is_selected {
                            9.0
                        } else if is_hovered {
                            8.0
                        } else {
                            6.5
                        });

                        if is_selected || is_hovered {
                            let glow_r = radius + px(if is_selected { 6.0 } else { 4.5 });
                            let glow_bounds = Bounds {
                                origin: point(center.x - glow_r, center.y - glow_r),
                                size: size(glow_r * 2.0, glow_r * 2.0),
                            };
                            window.paint_quad(
                                fill(
                                    glow_bounds,
                                    theme::accent().opacity(if is_selected { 0.30 } else { 0.20 }),
                                )
                                .corner_radii(glow_r),
                            );
                        }

                        let node_bounds = Bounds {
                            origin: point(center.x - radius, center.y - radius),
                            size: size(radius * 2.0, radius * 2.0),
                        };

                        painted_hits.push((slug.clone(), node_bounds, sx, sy));

                        let node_bg = if is_selected || is_hovered {
                            theme::accent()
                        } else if has_dead_link {
                            theme::error().into()
                        } else if is_orphan {
                            theme::warning().into()
                        } else {
                            theme::info().into()
                        };

                        window.paint_quad(fill(node_bounds, node_bg).corner_radii(radius));
                    }
                }

                if let Ok(mut cell) = hit_positions_for_paint.try_borrow_mut() {
                    *cell = painted_hits;
                }
            },
        )
        .size_full();

        let hit_positions_for_up = hit_positions_cell.clone();
        let hit_positions_for_move = hit_positions_cell.clone();
        let canvas_center_for_scroll = canvas_center_cell.clone();

        let hud_focus_node = hovered_note
            .as_ref()
            .or(selected_note.as_ref())
            .and_then(|slug| cached.nodes.get(slug));

        let canvas_hud_overlay = h_flex()
            .absolute()
            .top_2()
            .right_3()
            .max_w(px(360.0))
            .px_2p5()
            .py_1()
            .rounded_md()
            .bg(theme::bg_surface().opacity(0.85))
            .border_1()
            .border_color(theme::glass_highlight())
            .text_xs()
            .text_color(if hud_focus_node.is_some() {
                theme::text()
            } else {
                theme::text_muted()
            })
            .truncate()
            .child(if let Some(node) = hud_focus_node {
                node.title.clone()
            } else {
                "Scroll to zoom · Drag to pan · Click node to inspect".to_string()
            });

        let canvas_container = div()
            .id("nucleus-graph-canvas-viewport")
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .cursor_grab()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _window, _cx| {
                    this.nucleus_graph_last_mouse = Some(ev.position);
                    this.nucleus_graph_drag_moved = false;
                }),
            )
            .on_mouse_move(cx.listener(move |this, ev: &gpui::MouseMoveEvent, _window, cx| {
                this.nucleus_graph_canvas_center.set(canvas_center_cell.get());
                if ev.pressed_button == Some(MouseButton::Left) {
                    if let Some(last_pos) = this.nucleus_graph_last_mouse {
                        let dx = f32::from(ev.position.x - last_pos.x);
                        let dy = f32::from(ev.position.y - last_pos.y);
                        if this.nucleus_graph_drag_moved || (dx * dx + dy * dy) > 4.0 {
                            this.nucleus_graph_drag_moved = true;
                            this.nucleus_graph_pan.0 += dx;
                            this.nucleus_graph_pan.1 += dy;
                            this.nucleus_graph_last_mouse = Some(ev.position);
                            cx.notify();
                        }
                    } else {
                        this.nucleus_graph_last_mouse = Some(ev.position);
                    }
                } else {
                    this.nucleus_graph_last_mouse = None;
                    let mx = f32::from(ev.position.x);
                    let my = f32::from(ev.position.y);
                    let mut closest_hover: Option<String> = None;
                    let mut min_dist_sq = 18.0 * 18.0;
                    for (slug, _bounds, sx, sy) in hit_positions_for_move.borrow().iter() {
                        let dx = *sx - mx;
                        let dy = *sy - my;
                        let d_sq = dx * dx + dy * dy;
                        if d_sq < min_dist_sq {
                            min_dist_sq = d_sq;
                            closest_hover = Some(slug.clone());
                        }
                    }
                    if this.nucleus_graph_hovered_note != closest_hover {
                        this.nucleus_graph_hovered_note = closest_hover;
                        cx.notify();
                    }
                }
            }))
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseUpEvent, _window, cx| {
                    this.nucleus_graph_last_mouse = None;
                    if !this.nucleus_graph_drag_moved {
                        let click_pos = ev.position;
                        let click_x = f32::from(click_pos.x);
                        let click_y = f32::from(click_pos.y);

                        let mut closest_slug: Option<String> = None;
                        let mut min_dist_sq = 28.0 * 28.0;

                        for (slug, _bounds, sx, sy) in hit_positions_for_up.borrow().iter() {
                            let dx = *sx - click_x;
                            let dy = *sy - click_y;
                            let d_sq = dx * dx + dy * dy;
                            if d_sq < min_dist_sq {
                                min_dist_sq = d_sq;
                                closest_slug = Some(slug.clone());
                            }
                        }

                        if let Some(slug) = closest_slug {
                            this.visualizer_selected_note = Some(slug);
                            cx.notify();
                        }
                    }
                    this.nucleus_graph_drag_moved = false;
                }),
            )
            .on_scroll_wheel(cx.listener(move |this, ev: &gpui::ScrollWheelEvent, _window, cx| {
                cx.stop_propagation();
                let center = canvas_center_for_scroll.get();
                this.nucleus_graph_canvas_center.set(center);
                let delta = match ev.delta {
                    gpui::ScrollDelta::Lines(d) => d.y * 0.15,
                    gpui::ScrollDelta::Pixels(d) => f32::from(d.y) * 0.0025,
                };
                let cursor_pos = (f32::from(ev.position.x), f32::from(ev.position.y));
                let center_ref = if center.0 == 0.0 && center.1 == 0.0 {
                    cursor_pos
                } else {
                    center
                };
                let (new_zoom, new_pan) = apply_nucleus_zoom_at_cursor(
                    this.nucleus_graph_zoom,
                    this.nucleus_graph_pan,
                    delta,
                    cursor_pos,
                    center_ref,
                );
                this.nucleus_graph_zoom = new_zoom;
                this.nucleus_graph_pan = new_pan;
                cx.notify();
            }))
            .child(canvas_elem)
            .child(canvas_hud_overlay);

        let orphans_for_bulk = cached.orphaned_notes.clone();
        let dead_links_for_bulk = cached.dead_links.clone();
        let repo_root_for_orphans = repo_root.clone();
        let repo_root_for_delete_orphans = repo_root.clone();
        let repo_root_for_dead = repo_root.clone();

        let filter_pills = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .flex_wrap()
            .gap_1p5()
            .px_3()
            .py_1p5()
            .border_t_1()
            .border_color(theme::glass_highlight())
            .bg(theme::bg_surface().opacity(0.60))
            .child(
                h_flex()
                    .gap_1p5()
                    .items_center()
                    .child(
                        div()
                            .id("nucleus-filter-all")
                            .px_2p5()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(if is_all_filter { theme::glass_highlight().into() } else { theme::tab_bar_bg() })
                            .text_xs()
                            .font_weight(if is_all_filter { gpui::FontWeight::BOLD } else { gpui::FontWeight::MEDIUM })
                            .text_color(if is_all_filter { theme::accent() } else { theme::text_muted() })
                            .child("All Notes")
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.visualizer_nucleus_filter = None;
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("nucleus-filter-orphans")
                            .px_2p5()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(if is_orphan_filter { theme::glass_highlight().into() } else { theme::tab_bar_bg() })
                            .text_xs()
                            .font_weight(if is_orphan_filter { gpui::FontWeight::BOLD } else { gpui::FontWeight::MEDIUM })
                            .text_color(if is_orphan_filter { theme::warning() } else { theme::text_muted() })
                            .child(format!("Orphans ({})", cached.orphaned_notes.len()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.visualizer_nucleus_filter = Some("orphans".to_string());
                                cx.notify();
                            })),
                    )
                    .child(
                        div()
                            .id("nucleus-filter-dead-links")
                            .px_2p5()
                            .py_1()
                            .rounded_full()
                            .cursor_pointer()
                            .bg(if is_dead_link_filter { theme::glass_highlight().into() } else { theme::tab_bar_bg() })
                            .text_xs()
                            .font_weight(if is_dead_link_filter { gpui::FontWeight::BOLD } else { gpui::FontWeight::MEDIUM })
                            .text_color(if is_dead_link_filter { theme::error() } else { theme::text_muted() })
                            .child(format!("Dead Links ({})", cached.dead_links.len()))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.visualizer_nucleus_filter = Some("dead_links".to_string());
                                cx.notify();
                            })),
                    ),
            )
            .when(is_orphan_filter && !orphans_for_bulk.is_empty(), |bar| {
                let orphans_to_index = orphans_for_bulk.clone();
                let orphans_to_delete = orphans_for_bulk.clone();
                bar.child(
                    h_flex()
                        .gap_1p5()
                        .items_center()
                        .child(
                            div()
                                .id("nucleus-index-all-orphans")
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .cursor_pointer()
                                .bg(theme::accent().opacity(0.15))
                                .border_1()
                                .border_color(theme::accent().opacity(0.4))
                                .text_xs()
                                .text_color(theme::accent())
                                .child("+ Index All Orphans")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let nucleus_dir = repo_root_for_orphans.join(".hadron/nucleus");
                                    for slug in &orphans_to_index {
                                        let _ = index_orphan_note(&nucleus_dir, slug);
                                    }
                                    *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build_and_save(&repo_root_for_orphans));
                                    cx.notify();
                                })),
                        )
                        .child(
                            div()
                                .id("nucleus-delete-all-orphans")
                                .px_2()
                                .py_0p5()
                                .rounded_md()
                                .cursor_pointer()
                                .bg(theme::error().opacity(0.15))
                                .border_1()
                                .border_color(theme::error().opacity(0.4))
                                .text_xs()
                                .text_color(theme::error())
                                .child("Delete All Orphans")
                                .on_click(cx.listener(move |this, _, _, cx| {
                                    let nucleus_dir = repo_root_for_delete_orphans.join(".hadron/nucleus");
                                    for slug in &orphans_to_delete {
                                        let _ = delete_orphan_note(&nucleus_dir, slug);
                                    }
                                    this.visualizer_selected_note = None;
                                    *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build_and_save(&repo_root_for_delete_orphans));
                                    cx.notify();
                                })),
                        ),
                )
            })
            .when(is_dead_link_filter && !dead_links_for_bulk.is_empty(), |bar| {
                bar.child(
                    div()
                        .id("nucleus-strip-all-dead-links")
                        .px_2()
                        .py_0p5()
                        .rounded_md()
                        .cursor_pointer()
                        .bg(theme::error().opacity(0.15))
                        .border_1()
                        .border_color(theme::error().opacity(0.4))
                        .text_xs()
                        .text_color(theme::error())
                        .child("Unlink All Dead Links")
                        .on_click(cx.listener(move |this, _, _, cx| {
                            let nucleus_dir = repo_root_for_dead.join(".hadron/nucleus");
                            for (src, tgt) in &dead_links_for_bulk {
                                let _ = remove_dead_link_from_note(&nucleus_dir, src, tgt);
                            }
                            *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build_and_save(&repo_root_for_dead));
                            cx.notify();
                        })),
                )
            });

        let note_chips_dock = v_flex()
            .id("nucleus-notes-dock-scroll")
            .w_full()
            .max_h(px(140.0))
            .overflow_y_scroll()
            .child(
                h_flex()
                    .id("nucleus-notes-dock")
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .flex_wrap()
                    .px_3()
                    .py_2()
                    .gap_1p5()
                    .bg(theme::bg_surface().opacity(0.40))
                    .children(filtered_notes.iter().map(|n| {
                        let slug = n.slug.clone();
                        let is_sel = selected_note.as_deref() == Some(&slug);
                        let is_orphan = cached.orphaned_notes.contains(&slug);
                        let has_dead_link = dead_sources.contains(&slug);

                        let chip_bg = if is_sel {
                            theme::tab_bar_bg()
                        } else if has_dead_link {
                            theme::error().opacity(0.15)
                        } else if is_orphan {
                            theme::warning().opacity(0.15)
                        } else {
                            theme::tab_bar_bg().opacity(0.60)
                        };

                        let chip_border = if is_sel {
                            theme::accent()
                        } else if has_dead_link {
                            theme::error().opacity(0.40)
                        } else if is_orphan {
                            theme::warning().opacity(0.40)
                        } else {
                            theme::glass_highlight().into()
                        };

                        h_flex()
                            .id(SharedString::from(format!("nucleus-chip-{}", slug)))
                            .items_center()
                            .gap_1p5()
                            .px_2p5()
                            .py_1()
                            .rounded_md()
                            .cursor_pointer()
                            .bg(chip_bg)
                            .border_1()
                            .border_color(chip_border)
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if is_sel { theme::text() } else { theme::text_secondary() })
                                    .truncate()
                                    .max_w(px(140.0))
                                    .child(n.slug.clone()),
                            )
                            .when(is_orphan, |chip| {
                                chip.child(
                                    div()
                                        .flex_shrink_0()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(theme::warning().opacity(0.2))
                                        .text_xs()
                                        .text_color(theme::warning())
                                        .child("orphan"),
                                )
                            })
                            .when(has_dead_link, |chip| {
                                chip.child(
                                    div()
                                        .flex_shrink_0()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(theme::error().opacity(0.2))
                                        .text_xs()
                                        .text_color(theme::error())
                                        .child("dead link"),
                                )
                            })
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.visualizer_selected_note = Some(slug.clone());
                                cx.notify();
                            }))
                    }))
            );

        // Selected Note Inspector Panel
        let selected_note_panel = selected_note.as_ref().and_then(|slug| cached.nodes.get(slug)).map(|n| {
            let n_slug = n.slug.clone();
            let note_path = repo_root.join(".hadron/nucleus").join(&n.path);
            let content_preview = std::fs::read_to_string(&note_path).unwrap_or_else(|_| "Note file content unavailable".to_string());
            let is_orphan = cached.orphaned_notes.contains(&n_slug);
            let note_dead_targets: Vec<String> = cached
                .dead_links
                .iter()
                .filter(|(src, _)| src == &n_slug)
                .map(|(_, tgt)| tgt.clone())
                .collect();
            let repo_for_index = repo_root.clone();
            let repo_for_delete = repo_root.clone();
            let slug_for_index = n_slug.clone();
            let slug_for_delete = n_slug.clone();

            v_flex()
                .w_full()
                .p_3()
                .gap_2()
                .bg(theme::term_bg())
                .border_t_1()
                .border_color(theme::glass_highlight())
                .child(
                    h_flex()
                        .w_full()
                        .justify_between()
                        .items_center()
                        .child(
                            div()
                                .text_sm()
                                .font_weight(gpui::FontWeight::BOLD)
                                .text_color(theme::accent())
                                .child(n.slug.clone()),
                        )
                        .child(
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(n.path.clone()),
                                )
                                .child(
                                    div()
                                        .id("close-nucleus-note-inspector")
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .hover(|s| s.text_color(theme::text()))
                                        .child("✕")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.visualizer_selected_note = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(theme::text())
                        .child(n.title.clone()),
                )
                .child(
                    h_flex()
                        .gap_3()
                        .items_center()
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(format!("Incoming: {} link(s)", n.incoming_links.len()))
                        .child(format!("Outgoing: {} link(s)", n.outgoing_links.len())),
                )
                .when(is_orphan, |panel| {
                    panel.child(
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .gap_2()
                            .items_center()
                            .p_2()
                            .rounded_md()
                            .bg(theme::warning().opacity(0.10))
                            .border_1()
                            .border_color(theme::warning().opacity(0.35))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::warning())
                                    .child("Orphan Note:"),
                            )
                            .child(
                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(theme::bg_surface())
                                    .border_1()
                                    .border_color(theme::glass_highlight())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::warning())
                                            .child(format!("[[{}]]", n_slug)),
                                    )
                                    .child(
                                        div()
                                            .id("nucleus-action-index-orphan")
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .cursor_pointer()
                                            .bg(theme::accent().opacity(0.18))
                                            .text_xs()
                                            .text_color(theme::accent())
                                            .child("+ Add to Index")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let nucleus_dir = repo_for_index.join(".hadron/nucleus");
                                                let _ = index_orphan_note(&nucleus_dir, &slug_for_index);
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build_and_save(&repo_for_index));
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("nucleus-action-delete-orphan")
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .cursor_pointer()
                                            .bg(theme::error().opacity(0.18))
                                            .text_xs()
                                            .text_color(theme::error())
                                            .child("Delete Orphan")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let nucleus_dir = repo_for_delete.join(".hadron/nucleus");
                                                let _ = delete_orphan_note(&nucleus_dir, &slug_for_delete);
                                                this.visualizer_selected_note = None;
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build_and_save(&repo_for_delete));
                                                cx.notify();
                                            })),
                                    ),
                            ),
                    )
                })
                .when(!note_dead_targets.is_empty(), |panel| {
                    let src_slug = n_slug.clone();
                    let repo_for_links = repo_root.clone();
                    panel.child(
                        h_flex()
                            .w_full()
                            .flex_wrap()
                            .gap_2()
                            .items_center()
                            .p_2()
                            .rounded_md()
                            .bg(theme::error().opacity(0.10))
                            .border_1()
                            .border_color(theme::error().opacity(0.35))
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::error())
                                    .child("Dead Links:"),
                            )
                            .children(note_dead_targets.into_iter().map(move |target| {
                                let src_for_unlink = src_slug.clone();
                                let tgt_for_unlink = target.clone();
                                let tgt_for_stub = target.clone();
                                let repo_unlink = repo_for_links.clone();
                                let repo_stub = repo_for_links.clone();

                                h_flex()
                                    .gap_1p5()
                                    .items_center()
                                    .px_2()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(theme::bg_surface())
                                    .border_1()
                                    .border_color(theme::glass_highlight())
                                    .child(
                                        div()
                                            .text_xs()
                                            .text_color(theme::error())
                                            .child(format!("[[{}]]", target)),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("nucleus-unlink-{}-{}", src_for_unlink, target)))
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .cursor_pointer()
                                            .bg(theme::error().opacity(0.18))
                                            .text_xs()
                                            .text_color(theme::error())
                                            .child("Unlink")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let nucleus_dir = repo_unlink.join(".hadron/nucleus");
                                                let _ = remove_dead_link_from_note(&nucleus_dir, &src_for_unlink, &tgt_for_unlink);
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build_and_save(&repo_unlink));
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id(SharedString::from(format!("nucleus-stub-{}-{}", src_slug, target)))
                                            .px_1p5()
                                            .py_0p5()
                                            .rounded_sm()
                                            .cursor_pointer()
                                            .bg(theme::accent().opacity(0.18))
                                            .text_xs()
                                            .text_color(theme::accent())
                                            .child("+ Create Stub")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let nucleus_dir = repo_stub.join(".hadron/nucleus");
                                                let _ = create_stub_note(&nucleus_dir, &tgt_for_stub);
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build_and_save(&repo_stub));
                                                cx.notify();
                                            })),
                                    )
                            })),
                    )
                })
                .child(
                    div()
                        .id(SharedString::from(format!("nucleus-preview-{}", n.slug)))
                        .w_full()
                        .max_h(px(100.0))
                        .overflow_y_scroll()
                        .p_2()
                        .rounded_md()
                        .bg(theme::bg_surface())
                        .border_1()
                        .border_color(theme::glass_highlight())
                        .text_xs()
                        .text_color(theme::text_secondary())
                        .child(content_preview),
                )
        });

        v_flex()
            .id("nucleus-graph-container")
            .size_full()
            .bg(theme::canvas_base())
            .child(header)
            .child(canvas_container)
            .child(filter_pills)
            .child(note_chips_dock)
            .children(selected_note_panel)
            .into_any_element()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_force_directed_graph_repulsion() {
        let mut graph = ForceDirectedGraph::new();
        // Place two nodes very close together
        graph.add_node("note-a", 0.0, 0.0);
        graph.add_node("note-b", 1.0, 0.0);
        graph.add_edge("note-a", "note-b");

        graph.step_simulation(100.0, 0.0, 0.9);

        let na = &graph.nodes["note-a"];
        let nb = &graph.nodes["note-b"];

        // Repulsion should push them further apart along the x axis
        assert!(na.x < 0.0);
        assert!(nb.x > 1.0);
    }

    #[test]
    fn test_remove_dead_link_from_markdown_strips_wiki_and_md_links() {
        let src = "See [[missing-note]] and [Missing](notes/missing-note.md) for details, keep [[valid-note]].";
        let cleaned = remove_dead_link_from_markdown(src, "missing-note");
        assert_eq!(
            cleaned,
            "See `missing-note` and Missing for details, keep [[valid-note]]."
        );
        let remaining_links = hadron_forge::nucleus_graph::extract_note_links(&cleaned);
        assert_eq!(remaining_links, vec!["valid-note".to_string()]);
    }

    #[test]
    fn test_orphan_and_dead_link_remediation_helpers() {
        let tmp = tempfile::tempdir().unwrap();
        let nucleus_dir = tmp.path().join(".hadron/nucleus");
        let notes_dir = nucleus_dir.join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        std::fs::write(nucleus_dir.join("index.md"), "# Index\n").unwrap();
        std::fs::write(
            notes_dir.join("orphan-a.md"),
            "---\nname: orphan-a\ndescription: First orphan lesson\n---\nLinks to [[missing-target]].\n",
        )
        .unwrap();
        std::fs::write(
            notes_dir.join("orphan-b.md"),
            "---\nname: orphan-b\ndescription: Second orphan\n---\nStandalone.\n",
        )
        .unwrap();

        let (nodes, dead_links) = hadron_forge::nucleus_graph::build_nucleus_graph(&nucleus_dir);
        assert_eq!(nodes.len(), 2);
        assert_eq!(dead_links, vec![("orphan-a".to_string(), "missing-target".to_string())]);
        assert!(nodes["orphan-b"].incoming_links.is_empty());

        // 1. Index orphan-b -> no longer orphan
        index_orphan_note(&nucleus_dir, "orphan-b").unwrap();
        let (nodes, _) = hadron_forge::nucleus_graph::build_nucleus_graph(&nucleus_dir);
        assert_eq!(nodes["orphan-b"].incoming_links, vec!["index".to_string()]);

        // 2. Create stub for missing-target -> dead link resolved and orphan-a now has outgoing link to valid note
        create_stub_note(&nucleus_dir, "missing-target").unwrap();
        let (nodes, dead_links) = hadron_forge::nucleus_graph::build_nucleus_graph(&nucleus_dir);
        assert!(dead_links.is_empty());
        assert!(nodes.contains_key("missing-target"));

        // 3. Delete orphan-b
        delete_orphan_note(&nucleus_dir, "orphan-b").unwrap();
        let (nodes, _) = hadron_forge::nucleus_graph::build_nucleus_graph(&nucleus_dir);
        assert!(!nodes.contains_key("orphan-b"));
    }

    #[test]
    fn test_compute_nucleus_projection_distributes_across_viewport() {
        let mut positions = HashMap::new();
        for i in 0..240 {
            let r = 22.0 * ((i + 1) as f32).sqrt();
            let angle = (i as f32) * 2.399_963_1;
            positions.insert(
                format!("note-{i}"),
                (400.0 + r * angle.cos(), 300.0 + r * angle.sin()),
            );
        }

        let (cx_pt, cy_pt, center_orig_x, center_orig_y, scale_x, scale_y) =
            compute_nucleus_projection(&positions, 0.0, 0.0, 900.0, 400.0);

        let mut min_sy = f32::MAX;
        let mut max_sy = f32::MIN;
        for &(x, y) in positions.values() {
            let sx = cx_pt + (x - center_orig_x) * scale_x;
            let sy = cy_pt + (y - center_orig_y) * scale_y;
            assert!(sx >= 20.0 && sx <= 880.0, "sx out of viewport bounds: {sx}");
            assert!(sy >= 20.0 && sy <= 380.0, "sy out of viewport bounds: {sy}");
            min_sy = min_sy.min(sy);
            max_sy = max_sy.max(sy);
        }
        assert!(max_sy - min_sy > 300.0, "Nodes should span vertical viewport height");
    }

    #[test]
    fn test_nucleus_zoom_and_cursor_anchored_pan() {
        let mut positions = HashMap::new();
        positions.insert("center".to_string(), (400.0, 300.0));
        positions.insert("right".to_string(), (500.0, 300.0));
        positions.insert("left".to_string(), (300.0, 200.0));

        let canvas_center = (450.0, 200.0);
        let cursor = (520.0, 230.0);

        // Project "right" before zoom
        let (cx1, cy1, ox1, oy1, sx1, sy1) =
            compute_nucleus_projection_with_view(&positions, 0.0, 0.0, 900.0, 400.0, 1.0, 0.0, 0.0);
        let target_screen_x = cx1 + (500.0 - ox1) * sx1;
        let target_screen_y = cy1 + (300.0 - oy1) * sy1;

        // Place cursor directly over the "right" node and zoom in by +50%
        let (new_zoom, (pan_x, pan_y)) = apply_nucleus_zoom_at_cursor(
            1.0,
            (0.0, 0.0),
            0.5,
            (target_screen_x, target_screen_y),
            canvas_center,
        );
        assert!((new_zoom - 1.5).abs() < 1e-4);

        let (cx2, cy2, ox2, oy2, sx2, sy2) = compute_nucleus_projection_with_view(
            &positions, 0.0, 0.0, 900.0, 400.0, new_zoom, pan_x, pan_y,
        );
        // Scale should increase by 1.5x, separating tightly clustered nodes
        assert!((sx2 - sx1 * 1.5).abs() < 1e-4);
        assert!((sy2 - sy1 * 1.5).abs() < 1e-4);

        // And the node under the cursor stays at the exact same screen coordinates
        let after_screen_x = cx2 + (500.0 - ox2) * sx2;
        let after_screen_y = cy2 + (300.0 - oy2) * sy2;
        assert!((after_screen_x - target_screen_x).abs() < 1e-3);
        assert!((after_screen_y - target_screen_y).abs() < 1e-3);

        // Clamping bounds (0.5x to 8.0x)
        let (max_z, _) = apply_nucleus_zoom_at_cursor(7.5, (pan_x, pan_y), 0.5, cursor, canvas_center);
        assert_eq!(max_z, NUCLEUS_ZOOM_MAX);
        let (min_z, _) = apply_nucleus_zoom_at_cursor(0.55, (pan_x, pan_y), -0.5, cursor, canvas_center);
        assert_eq!(min_z, NUCLEUS_ZOOM_MIN);
    }

    #[test]
    fn test_filter_nucleus_nodes_isolates_orphans_and_dead_links() {
        let tmp = tempfile::tempdir().unwrap();
        let nucleus_dir = tmp.path().join(".hadron/nucleus");
        let notes_dir = nucleus_dir.join("notes");
        std::fs::create_dir_all(&notes_dir).unwrap();
        std::fs::write(
            nucleus_dir.join("index.md"),
            "- [connected-note](notes/connected-note.md) — Connected\n- [dead-link-note](notes/dead-link-note.md) — Has broken link\n",
        )
        .unwrap();
        std::fs::write(
            notes_dir.join("connected-note.md"),
            "---\nname: connected-note\ndescription: Connected\n---\nHealthy note.\n",
        )
        .unwrap();
        std::fs::write(
            notes_dir.join("dead-link-note.md"),
            "---\nname: dead-link-note\ndescription: Has broken link\n---\nLinks to [[missing-target]].\n",
        )
        .unwrap();
        std::fs::write(
            notes_dir.join("orphan-note.md"),
            "---\nname: orphan-note\ndescription: Unlinked orphan\n---\nStandalone.\n",
        )
        .unwrap();

        let cached = CachedNucleusGraph::build(tmp.path());

        let all = filter_nucleus_nodes(&cached, None);
        assert_eq!(all.len(), 3);

        let orphans = filter_nucleus_nodes(&cached, Some("orphans"));
        let orphan_slugs: Vec<String> = orphans.into_iter().map(|n| n.slug).collect();
        assert_eq!(orphan_slugs, vec!["orphan-note".to_string()]);

        let dead = filter_nucleus_nodes(&cached, Some("dead_links"));
        let dead_slugs: Vec<String> = dead.into_iter().map(|n| n.slug).collect();
        assert_eq!(dead_slugs, vec!["dead-link-note".to_string()]);
    }
}
