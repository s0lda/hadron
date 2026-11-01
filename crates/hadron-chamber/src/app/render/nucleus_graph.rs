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

        // 3. Apply velocity and damping
        for node in self.nodes.values_mut() {
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
    pub content_hash: u64,
    pub nodes: BTreeMap<String, hadron_forge::nucleus_graph::NucleusNode>,
    pub dead_links: Vec<(String, String)>,
    pub orphaned_notes: Vec<String>,
    pub force_graph: ForceDirectedGraph,
    pub positions: HashMap<String, (f32, f32)>,
}

impl CachedNucleusGraph {
    pub fn build(repo_root: &std::path::Path) -> Self {
        let nucleus_dir = repo_root.join(".hadron/nucleus");
        let (nodes, dead_links) = hadron_forge::nucleus_graph::build_nucleus_graph(&nucleus_dir);

        let mut force_graph = ForceDirectedGraph::new();
        let mut orphaned_notes = Vec::new();

        let num_nodes = nodes.len();
        let radius = (num_nodes as f32 * 35.0).max(180.0);

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
            force_graph.step_simulation(8000.0, 0.05, 0.85);
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
        let repo_root = crate::vcs::repo_root_of(&self.path);

        let mut cache_ref = self.cached_nucleus_graph.borrow_mut();
        if cache_ref.is_none() {
            *cache_ref = Some(CachedNucleusGraph::build(&repo_root));
        }
        let cached = cache_ref.as_ref().unwrap();

        let selected_note = self.visualizer_selected_note.clone();
        let filter_str = self.visualizer_nucleus_filter.as_deref().unwrap_or("").trim().to_lowercase();

        // Header Mode Switcher Capsule
        let mode_switcher = self.visualizer_mode_switcher_header(cx);

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
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(theme::warning().opacity(0.18))
                        .border_1()
                        .border_color(theme::warning().opacity(0.4))
                        .text_xs()
                        .text_color(theme::warning())
                        .child(format!("{} Orphans", cached.orphaned_notes.len())),
                )
            })
            .when(!cached.dead_links.is_empty(), |this| {
                this.child(
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(theme::error().opacity(0.18))
                        .border_1()
                        .border_color(theme::error().opacity(0.4))
                        .text_xs()
                        .text_color(theme::error())
                        .child(format!("{} Dead Links", cached.dead_links.len())),
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

        let hit_positions = positions_map.clone();

        let canvas_elem = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _cx| {
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                let cx_pt = f32::from(bounds.origin.x) + w / 2.0;
                let cy_pt = f32::from(bounds.origin.y) + h / 2.0;

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

                let range_x = (max_x - min_x).max(100.0);
                let range_y = (max_y - min_y).max(100.0);

                let scale_x = (w * 0.75) / range_x;
                let scale_y = (h * 0.70) / range_y;
                let scale = scale_x.min(scale_y).clamp(0.4, 2.5);

                let center_orig_x = (min_x + max_x) * 0.5;
                let center_orig_y = (min_y + max_y) * 0.5;

                let project = |x: f32, y: f32| -> (f32, f32) {
                    let sx = cx_pt + (x - center_orig_x) * scale;
                    let sy = cy_pt + (y - center_orig_y) * scale;
                    (sx, sy)
                };

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

                        let bounds = Bounds {
                            origin: point(center.x - radius, center.y - radius),
                            size: size(radius * 2.0, radius * 2.0),
                        };

                        let node_bg = if is_orphan {
                            theme::warning().into()
                        } else if is_selected {
                            theme::accent()
                        } else {
                            theme::info().into()
                        };

                        window.paint_quad(fill(bounds, node_bg).corner_radii(radius));
                    }
                }
            },
        );

        let canvas_container = div()
            .relative()
            .flex_1()
            .min_h_0()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, window, cx| {
                    let click_x = f32::from(ev.position.x);
                    let click_y = f32::from(ev.position.y);

                    let bounds = window.viewport_size();
                    let w = f32::from(bounds.width);
                    let h = f32::from(bounds.height);

                    let mut min_x = f32::MAX;
                    let mut max_x = f32::MIN;
                    let mut min_y = f32::MAX;
                    let mut max_y = f32::MIN;

                    for &(x, y) in hit_positions.values() {
                        min_x = min_x.min(x);
                        max_x = max_x.max(x);
                        min_y = min_y.min(y);
                        max_y = max_y.max(y);
                    }

                    let range_x = (max_x - min_x).max(100.0);
                    let range_y = (max_y - min_y).max(100.0);
                    let scale_x = (w * 0.75) / range_x;
                    let scale_y = (h * 0.70) / range_y;
                    let scale = scale_x.min(scale_y).clamp(0.4, 2.5);
                    let center_orig_x = (min_x + max_x) * 0.5;
                    let center_orig_y = (min_y + max_y) * 0.5;

                    let mut closest_slug: Option<String> = None;
                    let mut min_dist_sq = 28.0 * 28.0;

                    for (slug, &(x, y)) in &hit_positions {
                        let sx = (w / 2.0) + (x - center_orig_x) * scale;
                        let sy = (h / 2.0) + (y - center_orig_y) * scale;
                        let dx = sx - click_x;
                        let dy = sy - click_y;
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

        // Searchable Note Inspector Dock
        let filtered_notes: Vec<hadron_forge::nucleus_graph::NucleusNode> = cached
            .nodes
            .values()
            .filter(|n| {
                if filter_str.is_empty() {
                    true
                } else {
                    n.slug.to_lowercase().contains(&filter_str) || n.title.to_lowercase().contains(&filter_str)
                }
            })
            .cloned()
            .collect();

        let note_chips_dock = h_flex()
            .id("nucleus-notes-dock")
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .flex_wrap()
            .px_3()
            .py_2()
            .gap_1p5()
            .border_t_1()
            .border_color(theme::glass_highlight())
            .bg(theme::bg_surface().opacity(0.40))
            .children(filtered_notes.iter().take(24).map(|n| {
                let slug = n.slug.clone();
                let is_sel = selected_note.as_deref() == Some(&slug);
                let is_orphan = cached.orphaned_notes.contains(&slug);

                let chip_bg = if is_sel {
                    theme::tab_bar_bg()
                } else if is_orphan {
                    theme::warning().opacity(0.15)
                } else {
                    theme::tab_bar_bg().opacity(0.60)
                };

                let chip_border = if is_sel {
                    theme::accent()
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
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.visualizer_selected_note = Some(slug.clone());
                        cx.notify();
                    }))
            }));

        // Selected Note Inspector Panel
        let selected_note_panel = selected_note.as_ref().and_then(|slug| cached.nodes.get(slug)).map(|n| {
            let n_slug = n.slug.clone();
            let note_path = repo_root.join(&n.path);
            let content_preview = std::fs::read_to_string(&note_path).unwrap_or_else(|_| "Note file content unavailable".to_string());
            let is_orphan = cached.orphaned_notes.contains(&n_slug);

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
                                }),
                        )
                        .child(
                            div()
                                .text_xs()
                                .text_color(theme::text_muted())
                                .child(n.path.clone()),
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
            .child(note_chips_dock)
            .children(selected_note_panel)
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
}
