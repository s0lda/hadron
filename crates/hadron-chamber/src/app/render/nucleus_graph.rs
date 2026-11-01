//! 2D Force-Directed Nucleus Knowledge Graph.
//!
//! Provides a physical spring-embedder force simulation calculating dynamic 2D coordinates
//! for nucleus notes, invariants, and features connected by wiki-links, with Lavapipe caching
//! and interactive node selection.

use std::collections::{BTreeMap, HashMap, HashSet};
use serde::{Deserialize, Serialize};
use super::*;
use gpui::{canvas, fill, point, px, size, Bounds, MouseButton, PathBuilder};

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct ForceNode {
    pub slug: String,
    pub x: f32,
    pub y: f32,
    pub vx: f32,
    pub vy: f32,
}

#[allow(dead_code)]
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ForceEdge {
    pub source: String,
    pub target: String,
}

#[allow(dead_code)]
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ForceDirectedGraph {
    pub nodes: HashMap<String, ForceNode>,
    pub edges: Vec<ForceEdge>,
}

#[allow(dead_code)]
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

        // 2. Attractive forces along edges (Hooke's law)
        for edge in &self.edges {
            if let (Some(n1), Some(n2)) = (self.nodes.get(&edge.source), self.nodes.get(&edge.target)) {
                let dx = n2.x - n1.x;
                let dy = n2.y - n1.y;
                let dist = (dx * dx + dy * dy).sqrt().max(1.0);

                let force = dist * attraction_k;
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
#[derive(Debug, Clone)]
pub struct CachedNucleusGraph {
    #[allow(dead_code)]
    pub content_hash: u64,
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
    pub fn build(repo_root: &std::path::Path) -> Self {
        let nucleus_dir = repo_root.join(".hadron/nucleus");
        let (nodes, dead_links) = hadron_forge::nucleus_graph::build_nucleus_graph(&nucleus_dir);

        let mut force_graph = ForceDirectedGraph::new();
        let mut orphaned_notes = Vec::new();

        let num_nodes = nodes.len();
        let radius = (num_nodes as f32 * 24.0).clamp(120.0, 260.0);

        let mut i = 0;
        for (slug, node) in &nodes {
            let angle = (i as f32 / num_nodes.max(1) as f32) * std::f32::consts::TAU;
            let init_x = 400.0 + radius * angle.cos();
            let init_y = 300.0 + radius * angle.sin();
            force_graph.add_node(slug, init_x, init_y);
            i += 1;

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

        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        use std::hash::Hash;
        nodes.len().hash(&mut hasher);
        dead_links.len().hash(&mut hasher);
        let content_hash = std::hash::Hasher::finish(&hasher);

        Self {
            content_hash,
            nodes,
            dead_links,
            orphaned_notes,
            force_graph,
            positions,
        }
    }
}

impl super::Chamber {
    pub(super) fn nucleus_graph_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();

        if self.cached_nucleus_graph.borrow().is_none() {
            if !self.cached_nucleus_graph_loading.get() {
                self.cached_nucleus_graph_loading.set(true);
                let root_for_build = repo_root.clone();
                cx.spawn(async move |this, cx| {
                    let graph = cx
                        .background_executor()
                        .spawn(async move {
                            CachedNucleusGraph::build(&root_for_build)
                        })
                        .await;
                    let _ = this.update(cx, |chamber, cx| {
                        *chamber.cached_nucleus_graph.borrow_mut() = Some(graph);
                        chamber.cached_nucleus_graph_loading.set(false);
                        cx.notify();
                    });
                }).detach();
            }
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
        let filter_str = self.visualizer_nucleus_filter.as_deref().unwrap_or("").trim().to_lowercase();

        // Header Stats Badges
        let stats_hud = h_flex()
            .gap_2()
            .items_center()
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

        // 2D Node-Link Canvas Rendering
        let nodes_map = cached.nodes.clone();
        let edges_list = cached.force_graph.edges.clone();
        let positions_map = cached.positions.clone();
        let orphaned_set: HashSet<String> = cached.orphaned_notes.iter().cloned().collect();
        let dead_targets: HashSet<String> = cached.dead_links.iter().map(|(_, t)| t.clone()).collect();
        let sel_slug = selected_note.clone();

        let hit_positions_cell = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Bounds<Pixels>, f32, f32)>::new()));
        let hit_positions_for_paint = hit_positions_cell.clone();

        let canvas_elem = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _cx| {
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                let origin_x = f32::from(bounds.origin.x);
                let origin_y = f32::from(bounds.origin.y);
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

                let range_x = (max_x - min_x).max(60.0);
                let range_y = (max_y - min_y).max(60.0);

                let avail_w = (w - 48.0).max(40.0);
                let avail_h = (h - 48.0).max(40.0);
                let scale = (avail_w / range_x).min(avail_h / range_y).min(2.5);

                let center_orig_x = (min_x + max_x) * 0.5;
                let center_orig_y = (min_y + max_y) * 0.5;

                let project = |x: f32, y: f32| -> (f32, f32) {
                    let sx = cx_pt + (x - center_orig_x) * scale;
                    let sy = cy_pt + (y - center_orig_y) * scale;
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
                        let is_orphan = orphaned_set.contains(slug);

                        let radius = px(if is_selected { 9.0 } else { 6.5 });

                        if is_selected {
                            let glow_r = radius + px(6.0);
                            let glow_bounds = Bounds {
                                origin: point(center.x - glow_r, center.y - glow_r),
                                size: size(glow_r * 2.0, glow_r * 2.0),
                            };
                            window.paint_quad(
                                fill(glow_bounds, theme::accent().opacity(0.30)).corner_radii(glow_r),
                            );
                        }

                        let node_bounds = Bounds {
                            origin: point(center.x - radius, center.y - radius),
                            size: size(radius * 2.0, radius * 2.0),
                        };

                        painted_hits.push((slug.clone(), node_bounds, sx, sy));

                        let node_bg = if is_orphan {
                            theme::warning().into()
                        } else if is_selected {
                            theme::accent()
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
        );

        let hit_positions_for_click = hit_positions_cell.clone();

        let canvas_container = div()
            .relative()
            .flex_1()
            .min_h_0()
            .overflow_hidden()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _window, cx| {
                    let click_pos = ev.position;
                    let click_x = f32::from(click_pos.x);
                    let click_y = f32::from(click_pos.y);

                    let mut closest_slug: Option<String> = None;
                    let mut min_dist_sq = 28.0 * 28.0;

                    for (slug, _bounds, sx, sy) in hit_positions_for_click.borrow().iter() {
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
                }),
            )
            .child(canvas_elem);

        // Searchable Note Inspector Dock with Filter Pills
        let filter_mode = self.visualizer_nucleus_filter.as_deref().unwrap_or("all");
        let is_orphan_filter = filter_mode == "orphans";
        let is_dead_link_filter = filter_mode == "dead_links";
        let is_all_filter = !is_orphan_filter && !is_dead_link_filter;
        let dead_sources: HashSet<String> = cached.dead_links.iter().map(|(s, _)| s.clone()).collect();

        let filtered_notes: Vec<hadron_forge::nucleus_graph::NucleusNode> = cached
            .nodes
            .values()
            .filter(|n| {
                if is_orphan_filter && !cached.orphaned_notes.contains(&n.slug) {
                    return false;
                }
                if is_dead_link_filter && !dead_sources.contains(&n.slug) {
                    return false;
                }
                if filter_str.is_empty() || filter_str == "orphans" || filter_str == "dead_links" {
                    true
                } else {
                    n.slug.to_lowercase().contains(&filter_str) || n.title.to_lowercase().contains(&filter_str)
                }
            })
            .cloned()
            .collect();

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
                                    *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build(&repo_root_for_orphans));
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
                                    *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build(&repo_root_for_delete_orphans));
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
                            *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build(&repo_root_for_dead));
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
                            h_flex()
                                .gap_2()
                                .items_center()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(theme::accent())
                                        .child(n.slug.clone()),
                                )
                                .when(is_orphan, |this| {
                                    this.child(
                                        div()
                                            .px_2()
                                            .py_0p5()
                                            .rounded_sm()
                                            .bg(theme::warning().opacity(0.2))
                                            .text_xs()
                                            .text_color(theme::warning())
                                            .child("Orphan Note"),
                                    )
                                    .child(
                                        div()
                                            .id("nucleus-action-index-orphan")
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .bg(theme::accent().opacity(0.15))
                                            .border_1()
                                            .border_color(theme::accent().opacity(0.4))
                                            .text_xs()
                                            .text_color(theme::accent())
                                            .child("+ Add to Index")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let nucleus_dir = repo_for_index.join(".hadron/nucleus");
                                                let _ = index_orphan_note(&nucleus_dir, &slug_for_index);
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build(&repo_for_index));
                                                cx.notify();
                                            })),
                                    )
                                    .child(
                                        div()
                                            .id("nucleus-action-delete-orphan")
                                            .px_2()
                                            .py_0p5()
                                            .rounded_md()
                                            .cursor_pointer()
                                            .bg(theme::error().opacity(0.15))
                                            .border_1()
                                            .border_color(theme::error().opacity(0.4))
                                            .text_xs()
                                            .text_color(theme::error())
                                            .child("Delete Orphan")
                                            .on_click(cx.listener(move |this, _, _, cx| {
                                                let nucleus_dir = repo_for_delete.join(".hadron/nucleus");
                                                let _ = delete_orphan_note(&nucleus_dir, &slug_for_delete);
                                                this.visualizer_selected_note = None;
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build(&repo_for_delete));
                                                cx.notify();
                                            })),
                                    )
                                }),
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
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build(&repo_unlink));
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
                                                *this.cached_nucleus_graph.borrow_mut() = Some(CachedNucleusGraph::build(&repo_stub));
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
}
