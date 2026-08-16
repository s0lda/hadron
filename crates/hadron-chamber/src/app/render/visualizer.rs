//! Interactive 3D Sphere Topology visualizer for Hadron swarm.
//!
//! Projects quarks and their active execution/spawning topology onto a
//! mathematical 3D sphere using perspective projection, depth scaling,
//! animated excitation halos, and dynamic delegation links.

use std::f32::consts::PI;
use super::*;
use gpui::{canvas, fill, point, px, size, Bounds, Hsla, PathBuilder, Rgba};

/// 3D point in virtual sphere coordinate space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point3D {
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Spherical coordinates to Cartesian (radius, latitude theta [-pi/2, pi/2], longitude phi [0, 2pi]).
    pub fn from_spherical(radius: f32, theta: f32, phi: f32) -> Self {
        let cos_theta = theta.cos();
        Self {
            x: radius * cos_theta * phi.cos(),
            y: radius * theta.sin(),
            z: radius * cos_theta * phi.sin(),
        }
    }

    /// Rotate point around Y-axis (yaw) and X-axis (pitch).
    pub fn rotate(&self, yaw: f32, pitch: f32) -> Self {
        // Rotate around Y (yaw)
        let cos_y = yaw.cos();
        let sin_y = yaw.sin();
        let x1 = self.x * cos_y + self.z * sin_y;
        let y1 = self.y;
        let z1 = -self.x * sin_y + self.z * cos_y;

        // Rotate around X (pitch)
        let cos_p = pitch.cos();
        let sin_p = pitch.sin();
        let x2 = x1;
        let y2 = y1 * cos_p - z1 * sin_p;
        let z2 = y1 * sin_p + z1 * cos_p;

        Self { x: x2, y: y2, z: z2 }
    }

    /// Project 3D point to 2D screen coordinate with perspective depth.
    pub fn project(&self, center_x: f32, center_y: f32, distance: f32) -> (f32, f32, f32, f32) {
        let depth_offset = distance + 180.0;
        let scale = (distance / (depth_offset - self.z)).clamp(0.4, 2.2);
        let screen_x = center_x + self.x * scale;
        let screen_y = center_y - self.y * scale; // Invert Y for screen coords
        (screen_x, screen_y, self.z, scale)
    }
}

/// A projected node ready for depth-sorted rendering.
#[derive(Debug, Clone)]
pub struct ProjectedQuarkNode {
    pub id: String,
    pub name: String,
    pub model: String,
    pub color: Hsla,
    pub is_orchestrator: bool,
    pub is_enabled: bool,
    pub is_excited: bool,
    pub point_3d: Point3D,
    pub screen_x: f32,
    pub screen_y: f32,
    pub z_depth: f32,
    pub scale: f32,
    pub radius: f32,
}

impl super::Chamber {
    /// Render the interactive 3D Sphere Topology visualizer tab.
    pub(super) fn visualizer_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let now = chrono::Utc::now();
        let time_secs = (now.timestamp_millis() % 100_000) as f32 / 1000.0;

        // Auto-rotation angle progression
        let yaw = if self.visualizer_auto_spin {
            self.visualizer_yaw + time_secs * 0.35
        } else {
            self.visualizer_yaw
        };
        let pitch = self.visualizer_pitch;

        let roster = &self.view.roster;
        let total_quarks = roster.len();
        let live_dir = hadron_lattice::live::live_dir(&self.path);

        // Build 3D sphere nodes using Fibonacci sphere / harmonic distribution
        let sphere_radius = 110.0 * self.visualizer_zoom.clamp(0.6, 2.0);
        let mut nodes: Vec<ProjectedQuarkNode> = Vec::new();

        for (ix, row) in roster.iter().enumerate() {
            let is_orchestrator = matches!(row.flavor, Some(hadron_lattice::Flavor::Orchestrator));
            let color = self.color_for(&row.id);
            let resolved = self.resolve_identity(&row.id);

            let activity = hadron_lattice::live::read(
                &live_dir,
                &hadron_lattice::QuarkId::new(&row.id),
                now,
            );
            let is_excited = activity.is_some() || self.view.tasks.iter().any(|t| t.to == row.id || t.from == row.id);

            // Compute spherical coordinates:
            // Orchestrator sits at the apex (theta = ~1.1 rad)
            // Workers distributed evenly on latitude / longitude
            let (theta, phi) = if is_orchestrator {
                (1.1f32, 0.0f32)
            } else {
                let non_orch_count = total_quarks.saturating_sub(1).max(1);
                let idx = if is_orchestrator { 0 } else { ix };
                // Golden spiral on sphere
                let lat_frac = ((idx as f32 + 0.5) / non_orch_count as f32) * 2.0 - 1.0;
                let theta = lat_frac.clamp(-0.85, 0.7).asin();
                let phi = idx as f32 * 2.39996323; // Golden angle ~137.5 deg
                (theta, phi)
            };

            let p3d = Point3D::from_spherical(sphere_radius, theta, phi);
            let rotated = p3d.rotate(yaw, pitch);

            nodes.push(ProjectedQuarkNode {
                id: row.id.clone(),
                name: resolved.name,
                model: if row.model.is_empty() { "default".into() } else { row.model.clone() },
                color,
                is_orchestrator,
                is_enabled: row.enabled && row.adopted,
                is_excited,
                point_3d: rotated,
                screen_x: 0.0,
                screen_y: 0.0,
                z_depth: rotated.z,
                scale: 1.0,
                radius: if is_orchestrator { 8.5 } else { 6.5 },
            });
        }

        // Active swarm links from Orchestrator / Tasks
        let active_tasks: Vec<(String, String)> = self
            .view
            .tasks
            .iter()
            .map(|t| (t.from.clone(), t.to.clone()))
            .collect();

        let selected_quark = self.visualizer_selected_quark.clone();
        let auto_spin = self.visualizer_auto_spin;

        let active_count = roster.iter().filter(|r| r.enabled && r.adopted).count();
        let excited_count = nodes.iter().filter(|n| n.is_excited).count();

        let hud_summary = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .px_2()
                    .py_0p5()
                    .rounded_full()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::accent())
                    .child(format!("{active_count} Active Quarks")),
            )
            .when(excited_count > 0, |this| {
                this.child(
                    div()
                        .px_2()
                        .py_0p5()
                        .rounded_full()
                        .bg(gpui::rgb(0x22c55e).opacity(0.18))
                        .border_1()
                        .border_color(gpui::rgb(0x22c55e).opacity(0.4))
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(gpui::rgb(0x22c55e))
                        .child(format!("{excited_count} Excited")),
                )
            });

        let hud_controls = h_flex()
            .gap_2()
            .items_center()
            .child(
                div()
                    .id("vis-toggle-spin")
                    .px_2p5()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(if auto_spin { theme::accent() } else { theme::text_muted() })
                    .hover(|s| s.text_color(theme::text()))
                    .child(if auto_spin { "Auto-Spin: ON" } else { "Auto-Spin: OFF" })
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.visualizer_auto_spin = !this.visualizer_auto_spin;
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .id("vis-reset-cam")
                    .px_2p5()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(theme::tab_bar_bg())
                    .border_1()
                    .border_color(theme::glass_highlight())
                    .text_xs()
                    .text_color(theme::text_muted())
                    .hover(|s| s.text_color(theme::text()))
                    .child("Reset View")
                    .on_click(cx.listener(|this, _, _, cx| {
                        this.visualizer_yaw = 0.0;
                        this.visualizer_pitch = 0.25;
                        this.visualizer_zoom = 1.0;
                        this.visualizer_auto_spin = true;
                        this.visualizer_selected_quark = None;
                        cx.notify();
                    })),
            );

        // Header strip
        let header = h_flex()
            .w_full()
            .justify_between()
            .items_center()
            .px_4()
            .py_2()
            .child(hud_summary)
            .child(hud_controls);

        // Selected Quark Focus Card (bottom overlay)
        let selected_node_info = selected_quark.as_ref().and_then(|qid| {
            nodes.iter().find(|n| &n.id == qid).cloned()
        });

        let selected_hud = selected_node_info.map(|node| {
            let qid_for_info = node.id.clone();
            h_flex()
                .w_full()
                .justify_between()
                .items_center()
                .p_3()
                .rounded_lg()
                .bg(theme::term_bg())
                .border_1()
                .border_color(theme::glass_highlight())
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            div()
                                .size_3()
                                .rounded_full()
                                .bg(node.color),
                        )
                        .child(
                            v_flex()
                                .gap_0p5()
                                .child(
                                    div()
                                        .text_sm()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(theme::text())
                                        .child(node.name),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(format!("{} · {}", node.id, node.model)),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .gap_2()
                        .child(
                            text_button("vis-open-info", "View Details").on_click(cx.listener(
                                move |this, _, _window, cx| {
                                    this.info_panel = Some(qid_for_info.clone());
                                    this.info_tab = InfoTab::Identity;
                                    cx.notify();
                                },
                            )),
                        ),
                )
        });

        // 3D Canvas
        let canvas_elem = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _cx| {
                let w = bounds.size.width;
                let h = bounds.size.height;
                let cx_pt = bounds.origin.x + w / 2.0;
                let cy_pt = bounds.origin.y + h / 2.0;
                let distance = 380.0;

                // 1. Draw 3D wireframe latitude rings
                let lats = [-1.0f32, -0.5, 0.0, 0.5, 1.0];
                for &lat_ratio in &lats {
                    let lat_theta = lat_ratio * (PI * 0.35);
                    let mut builder = PathBuilder::stroke(px(1.0));
                    let mut first_point = true;

                    for step in 0..=36 {
                        let phi = (step as f32 / 36.0) * (2.0 * PI);
                        let p = Point3D::from_spherical(sphere_radius, lat_theta, phi).rotate(yaw, pitch);
                        let (sx, sy, _, _) = p.project(f32::from(cx_pt), f32::from(cy_pt), distance);

                        if first_point {
                            builder.move_to(point(px(sx), px(sy)));
                            first_point = false;
                        } else {
                            builder.line_to(point(px(sx), px(sy)));
                        }
                    }

                    if let Ok(path) = builder.build() {
                        let ring_color = gpui::rgb(0x38bdf8).opacity(0.12);
                        window.paint_path(path, ring_color);
                    }
                }

                // 2. Draw 3D wireframe longitude meridians
                for m in 0..8 {
                    let phi = (m as f32 / 8.0) * (2.0 * PI);
                    let mut builder = PathBuilder::stroke(px(1.0));
                    let mut first_point = true;

                    for step in 0..=36 {
                        let theta = (step as f32 / 36.0) * PI - (PI / 2.0);
                        let p = Point3D::from_spherical(sphere_radius, theta, phi).rotate(yaw, pitch);
                        let (sx, sy, _, _) = p.project(f32::from(cx_pt), f32::from(cy_pt), distance);

                        if first_point {
                            builder.move_to(point(px(sx), px(sy)));
                            first_point = false;
                        } else {
                            builder.line_to(point(px(sx), px(sy)));
                        }
                    }

                    if let Ok(path) = builder.build() {
                        let meridian_color = gpui::rgb(0x38bdf8).opacity(0.10);
                        window.paint_path(path, meridian_color);
                    }
                }

                // 3. Project and depth-sort Quark nodes
                let mut projected_nodes = nodes.clone();
                for node in &mut projected_nodes {
                    let (sx, sy, z, scale) = node.point_3d.project(f32::from(cx_pt), f32::from(cy_pt), distance);
                    node.screen_x = sx;
                    node.screen_y = sy;
                    node.z_depth = z;
                    node.scale = scale;
                }

                // Sort back-to-front (lowest z rendered first)
                projected_nodes.sort_by(|a, b| a.z_depth.partial_cmp(&b.z_depth).unwrap_or(std::cmp::Ordering::Equal));

                // 4. Draw delegation links between nodes
                for (from_id, to_id) in &active_tasks {
                    if let (Some(from_node), Some(to_node)) = (
                        projected_nodes.iter().find(|n| &n.id == from_id),
                        projected_nodes.iter().find(|n| &n.id == to_id),
                    ) {
                        let mut builder = PathBuilder::stroke(px(1.5));
                        builder.move_to(point(px(from_node.screen_x), px(from_node.screen_y)));
                        let mid_x = (from_node.screen_x + to_node.screen_x) / 2.0;
                        let mid_y = (from_node.screen_y + to_node.screen_y) / 2.0 - 25.0 * from_node.scale;
                        builder.cubic_bezier_to(
                            point(px(to_node.screen_x), px(to_node.screen_y)),
                            point(px(mid_x), px(mid_y)),
                            point(px(mid_x), px(mid_y)),
                        );

                        if let Ok(path) = builder.build() {
                            let link_color = gpui::rgb(0x06b6d4).opacity(0.45);
                            window.paint_path(path, link_color);
                        }
                    }
                }

                // 5. Draw Quark Nodes with depth scaling and active glowing halos
                for node in &projected_nodes {
                    let radius = px(node.radius * node.scale);
                    let center = point(px(node.screen_x), px(node.screen_y));
                    let bounds = Bounds {
                        origin: point(center.x - radius, center.y - radius),
                        size: size(radius * 2.0, radius * 2.0),
                    };

                    let depth_alpha = ((node.z_depth + sphere_radius) / (2.0 * sphere_radius)).clamp(0.25, 1.0);
                    let node_color = if node.is_enabled {
                        node.color.opacity(depth_alpha)
                    } else {
                        gpui::rgb(0x64748b).opacity(depth_alpha * 0.4).into()
                    };

                    // Active excitation halo ring
                    if node.is_excited {
                        let pulse = ((time_secs * 4.0).sin() * 0.2 + 0.8) as f32;
                        let halo_r = radius * (1.8 * pulse);
                        let halo_bounds = Bounds {
                            origin: point(center.x - halo_r, center.y - halo_r),
                            size: size(halo_r * 2.0, halo_r * 2.0),
                        };
                        window.paint_quad(
                            fill(halo_bounds, node.color.opacity(0.22 * depth_alpha)).corner_radii(halo_r),
                        );
                    }

                    // Orchestrator golden crown halo
                    if node.is_orchestrator {
                        let crown_r = radius + px(3.5);
                        let crown_bounds = Bounds {
                            origin: point(center.x - crown_r, center.y - crown_r),
                            size: size(crown_r * 2.0, crown_r * 2.0),
                        };
                        window.paint_quad(
                            fill(crown_bounds, gpui::rgb(0xf59e0b).opacity(0.35 * depth_alpha)).corner_radii(crown_r),
                        );
                    }

                    // Outer border
                    let border_r = radius + px(1.5);
                    let border_bounds = Bounds {
                        origin: point(center.x - border_r, center.y - border_r),
                        size: size(border_r * 2.0, border_r * 2.0),
                    };
                    window.paint_quad(
                        fill(border_bounds, theme::canvas_base()).corner_radii(border_r),
                    );

                    // Core sphere node
                    window.paint_quad(
                        fill(bounds, node_color).corner_radii(radius),
                    );
                }
            },
        )
        .size_full();

        v_flex()
            .id("visualizer-container")
            .size_full()
            .bg(theme::canvas_base())
            .child(header)
            .child(
                div()
                    .relative()
                    .flex_1()
                    .min_h_0()
                    .child(canvas_elem),
            )
            .children(selected_hud)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point3d_spherical_conversion() {
        let p_north = Point3D::from_spherical(100.0, PI / 2.0, 0.0);
        assert!((p_north.y - 100.0).abs() < 1e-3);
        assert!(p_north.x.abs() < 1e-3);
        assert!(p_north.z.abs() < 1e-3);

        let p_equator = Point3D::from_spherical(100.0, 0.0, 0.0);
        assert!((p_equator.x - 100.0).abs() < 1e-3);
        assert!(p_equator.y.abs() < 1e-3);
        assert!(p_equator.z.abs() < 1e-3);
    }

    #[test]
    fn point3d_rotation_preserves_radius() {
        let p = Point3D::new(50.0, 30.0, 40.0);
        let orig_dist = (p.x * p.x + p.y * p.y + p.z * p.z).sqrt();

        let rotated = p.rotate(0.75, -0.4);
        let rot_dist = (rotated.x * rotated.x + rotated.y * rotated.y + rotated.z * rotated.z).sqrt();

        assert!((orig_dist - rot_dist).abs() < 1e-3, "Rotation must preserve Euclidean radius");
    }

    #[test]
    fn point3d_perspective_projection_scales_with_depth() {
        let p_front = Point3D::new(10.0, 20.0, 50.0);
        let p_back = Point3D::new(10.0, 20.0, -50.0);

        let (_, _, _, scale_front) = p_front.project(200.0, 200.0, 400.0);
        let (_, _, _, scale_back) = p_back.project(200.0, 200.0, 400.0);

        assert!(scale_front > scale_back, "Front point must have larger perspective scale than back point");
    }
}
