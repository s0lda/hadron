//! Interactive 3D Sphere Topology visualizer for Hadron swarm.
//!
//! Projects quarks and their active execution/spawning topology onto a
//! mathematical 3D cosmic sphere using perspective projection, depth scaling,
//! neural constellation wireframe lattices, glowing singularity core,
//! traveling orbital photon belt, animated excitation halos, and dynamic delegation links.

use std::f32::consts::PI;
use std::sync::LazyLock;
use super::*;
use gpui::{canvas, fill, point, px, size, Bounds, Hsla, MouseButton, PathBuilder};
use crate::model::tasks::TaskState;

/// 3D point in virtual sphere coordinate space.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Point3D {
    pub x: f32,
    pub y: f32,
    pub z: f32,
}

impl Point3D {
    #[allow(dead_code)]
    pub const fn new(x: f32, y: f32, z: f32) -> Self {
        Self { x, y, z }
    }

    /// Scale point coordinates by a scalar factor.
    #[inline(always)]
    pub fn scale(&self, factor: f32) -> Self {
        Self {
            x: self.x * factor,
            y: self.y * factor,
            z: self.z * factor,
        }
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
        let cos_y = yaw.cos();
        let sin_y = yaw.sin();
        let x1 = self.x * cos_y + self.z * sin_y;
        let y1 = self.y;
        let z1 = -self.x * sin_y + self.z * cos_y;

        let cos_p = pitch.cos();
        let sin_p = pitch.sin();
        let x2 = x1;
        let y2 = y1 * cos_p - z1 * sin_p;
        let z2 = y1 * sin_p + z1 * cos_p;

        Self { x: x2, y: y2, z: z2 }
    }

    /// Rotate point around Z-axis (roll / celestial inclination tilt).
    pub fn rotate_z(&self, angle: f32) -> Self {
        let cos_a = angle.cos();
        let sin_a = angle.sin();
        Self {
            x: self.x * cos_a - self.y * sin_a,
            y: self.x * sin_a + self.y * cos_a,
            z: self.z,
        }
    }

    /// Project 3D point to 2D screen coordinate with perspective depth scaling.
    #[inline(always)]
    pub fn project(&self, center_x: f32, center_y: f32, distance: f32) -> (f32, f32, f32, f32) {
        let depth_offset = distance + 220.0;
        let scale = (distance / (depth_offset - self.z)).clamp(0.35, 2.5);
        let screen_x = center_x + self.x * scale;
        let screen_y = center_y - self.y * scale; // Invert Y for screen coordinates
        (screen_x, screen_y, self.z, scale)
    }

    /// Squared Euclidean distance between two 3D points.
    #[inline(always)]
    pub fn dist_sq(&self, other: &Point3D) -> f32 {
        let dx = self.x - other.x;
        let dy = self.y - other.y;
        let dz = self.z - other.z;
        dx * dx + dy * dy + dz * dz
    }
}

/// High-performance 3x3 rotation matrix for 3D sphere coordinate transformations.
///
/// Avoids calculating trigonometric functions (sin/cos) per point by computing the
/// composite Euler rotation matrix once per frame.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Rotation3D {
    m00: f32, m01: f32, m02: f32,
    m10: f32, m11: f32, m12: f32,
    m20: f32, m21: f32, m22: f32,
}

impl Rotation3D {
    /// Compute 3D rotation matrix for yaw (Y-axis) and pitch (X-axis).
    pub fn new(yaw: f32, pitch: f32) -> Self {
        let cy = yaw.cos();
        let sy = yaw.sin();
        let cp = pitch.cos();
        let sp = pitch.sin();
        Self {
            m00: cy,        m01: 0.0, m02: sy,
            m10: sy * sp,   m11: cp,  m12: -cy * sp,
            m20: -sy * cp,  m21: sp,  m22: cy * cp,
        }
    }

    /// Transform a 3D point using this rotation matrix.
    #[inline(always)]
    pub fn transform(&self, p: Point3D) -> Point3D {
        Point3D {
            x: self.m00 * p.x + self.m01 * p.y + self.m02 * p.z,
            y: self.m10 * p.x + self.m11 * p.y + self.m12 * p.z,
            z: self.m20 * p.x + self.m21 * p.y + self.m22 * p.z,
        }
    }
}

// -----------------------------------------------------------------------------
// Precomputed Static Topology & Geometry Tables
// -----------------------------------------------------------------------------

struct StarSeed {
    unit_p: Point3D,
    star_dist_mult: f32,
    seed: f32,
    color_type: u8,
}

static STAR_FIELD: LazyLock<[StarSeed; 48]> = LazyLock::new(|| {
    std::array::from_fn(|s_ix| {
        let seed = s_ix as f32;
        let star_dist_mult = 1.35 + (seed * 0.027);
        let star_theta = ((seed * 1.618).sin() * 0.9).asin();
        let star_phi = seed * 2.39996323;
        let unit_p = Point3D::from_spherical(star_dist_mult, star_theta, star_phi);
        StarSeed {
            unit_p,
            star_dist_mult,
            seed,
            color_type: (s_ix % 3) as u8,
        }
    })
});

const LATTICE_COUNT: usize = 64;

static LATTICE_UNIT_NODES: LazyLock<[Point3D; LATTICE_COUNT]> = LazyLock::new(|| {
    std::array::from_fn(|i| {
        let lat_frac = ((i as f32 + 0.5) / LATTICE_COUNT as f32) * 2.0 - 1.0;
        let theta = (lat_frac * 0.94).asin();
        let phi = i as f32 * 2.39996323;
        Point3D::from_spherical(1.0, theta, phi)
    })
});

static LATTICE_STATIC_EDGES: LazyLock<Vec<(usize, usize)>> = LazyLock::new(|| {
    let nodes = &*LATTICE_UNIT_NODES;
    let mut edges = Vec::with_capacity(160);
    let max_link_sq = 0.44 * 0.44;
    for i in 0..LATTICE_COUNT {
        for j in (i + 1)..LATTICE_COUNT {
            if nodes[i].dist_sq(&nodes[j]) < max_link_sq {
                edges.push((i, j));
            }
        }
    }
    edges
});

const LAT_STEPS: usize = 36;
const NUM_LATS: usize = 5;
const MERIDIAN_STEPS: usize = 36;
const NUM_MERIDIANS: usize = 8;
const BELT_STEPS: usize = 48;

static LATITUDE_UNIT_RINGS: LazyLock<[[Point3D; LAT_STEPS + 1]; NUM_LATS]> = LazyLock::new(|| {
    let lats = [-0.8f32, -0.4, 0.0, 0.4, 0.8];
    std::array::from_fn(|ring_idx| {
        let lat_theta = lats[ring_idx] * (PI * 0.42);
        std::array::from_fn(|step| {
            let phi = (step as f32 / LAT_STEPS as f32) * (2.0 * PI);
            Point3D::from_spherical(1.0, lat_theta, phi)
        })
    })
});

static LONGITUDE_UNIT_MERIDIANS: LazyLock<[[Point3D; MERIDIAN_STEPS + 1]; NUM_MERIDIANS]> = LazyLock::new(|| {
    std::array::from_fn(|m_idx| {
        let phi = (m_idx as f32 / NUM_MERIDIANS as f32) * (2.0 * PI);
        std::array::from_fn(|step| {
            let theta = (step as f32 / MERIDIAN_STEPS as f32) * PI - (PI / 2.0);
            Point3D::from_spherical(1.0, theta, phi)
        })
    })
});

static BELT_UNIT_RING: LazyLock<[Point3D; BELT_STEPS + 1]> = LazyLock::new(|| {
    let belt_tilt = 28.0 * (PI / 180.0);
    std::array::from_fn(|step| {
        let phi = (step as f32 / BELT_STEPS as f32) * (2.0 * PI);
        Point3D::from_spherical(1.18, 0.0, phi).rotate_z(belt_tilt)
    })
});

/// A projected node ready for depth-sorted rendering and click hit-testing.
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
    /// Render the interactive 3D Cosmic Sphere Topology visualizer tab.
    pub(super) fn visualizer_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let now = chrono::Utc::now();
        let time_secs = (now.timestamp_millis() % 1_000_000) as f32 / 1000.0;

        // Auto-rotation angle progression
        let yaw = if self.visualizer_auto_spin {
            self.visualizer_yaw + time_secs * 0.28
        } else {
            self.visualizer_yaw
        };
        let pitch = self.visualizer_pitch;

        // Precompute rotation matrices for the frame
        let rot = Rotation3D::new(yaw, pitch);

        let roster = &self.view.roster;
        let total_quarks = roster.len();
        let live_dir = hadron_lattice::live::live_dir(&self.path);

        // Calculate active and excited counts accurately:
        // A quark is excited ONLY if it currently has fresh activity (thinking/working/planning/speaking)
        // OR an in-flight working task (TaskState::Working). Completed/Done tasks do NOT excite.
        let mut excited_quark_ids = std::collections::HashSet::new();
        for row in roster.iter() {
            let activity = hadron_lattice::live::read(
                &live_dir,
                &hadron_lattice::QuarkId::new(&row.id),
                now,
            );
            let has_active_task = self.view.tasks.iter().any(|t| {
                (t.to == row.id || t.from == row.id) && t.state == TaskState::Working
            });

            if activity.is_some() || has_active_task {
                excited_quark_ids.insert(row.id.clone());
            }
        }

        // Active swarm links only for in-flight tasks
        let active_tasks: Vec<(String, String)> = self
            .view
            .tasks
            .iter()
            .filter(|t| t.state == TaskState::Working)
            .map(|t| (t.from.clone(), t.to.clone()))
            .collect();

        // Base sphere geometry (dynamic larger cosmic sphere scale)
        let zoom = self.visualizer_zoom.clamp(0.5, 3.0);
        let base_radius = 160.0 * zoom;

        // Build 3D sphere nodes using Fibonacci harmonic distribution
        let mut nodes: Vec<ProjectedQuarkNode> = Vec::with_capacity(total_quarks);
        for (ix, row) in roster.iter().enumerate() {
            let is_orchestrator = matches!(row.flavor, Some(hadron_lattice::Flavor::Orchestrator));
            let color = self.color_for(&row.id);
            let resolved = self.resolve_identity(&row.id);
            let is_excited = excited_quark_ids.contains(&row.id);

            // Compute spherical coordinates:
            // Orchestrator sits at apex crown (theta = ~1.12 rad)
            // Workers distributed harmoniously across the sphere surface
            let (theta, phi) = if is_orchestrator {
                (1.12f32, 0.0f32)
            } else {
                let non_orch_count = total_quarks.saturating_sub(1).max(1);
                let idx = if is_orchestrator { 0 } else { ix };
                let lat_frac = ((idx as f32 + 0.5) / non_orch_count as f32) * 2.0 - 1.0;
                let theta = (lat_frac * 0.75).asin();
                let phi = idx as f32 * 2.39996323; // Golden angle ~137.5 deg
                (theta, phi)
            };

            let p3d = Point3D::from_spherical(base_radius, theta, phi);
            let rotated = rot.transform(p3d);

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
                radius: if is_orchestrator { 9.5 } else { 7.0 },
            });
        }

        let selected_quark = self.visualizer_selected_quark.clone();
        let auto_spin = self.visualizer_auto_spin;
        let active_count = roster.iter().filter(|r| r.enabled && r.adopted).count();
        let excited_count = excited_quark_ids.len();

        // Top Status HUD with unambiguous stats and state chips
        let hud_summary = h_flex()
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
                    .child(format!("{active_count} Seated")),
            )
            .child(
                if excited_count > 0 {
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(gpui::rgb(0x22c55e).opacity(0.18))
                        .border_1()
                        .border_color(gpui::rgb(0x22c55e).opacity(0.45))
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(gpui::rgb(0x22c55e))
                        .child(
                            div()
                                .size_2()
                                .rounded_full()
                                .bg(gpui::rgb(0x22c55e)),
                        )
                        .child(format!("{excited_count} Excited"))
                } else {
                    div()
                        .flex()
                        .items_center()
                        .gap_1p5()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(theme::tab_bar_bg())
                        .border_1()
                        .border_color(theme::glass_highlight())
                        .text_xs()
                        .text_color(theme::text_muted())
                        .child(
                            div()
                                .size_2()
                                .rounded_full()
                                .bg(gpui::rgb(0x38bdf8).opacity(0.4)),
                        )
                        .child("All Quarks Idle")
                },
            );

        // HUD Controls (Auto-Spin, Reset, Zoom In/Out)
        let hud_controls = h_flex()
            .gap_1p5()
            .items_center()
            .child(
                div()
                    .id("vis-zoom-out")
                    .px_2()
                    .py_1()
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
                        this.visualizer_zoom = (this.visualizer_zoom - 0.15).clamp(0.5, 3.0);
                        cx.notify();
                    })),
            )
            .child(
                div()
                    .px_2()
                    .py_1()
                    .text_xs()
                    .text_color(theme::text_muted())
                    .child(format!("{:.0}%", zoom * 100.0)),
            )
            .child(
                div()
                    .id("vis-zoom-in")
                    .px_2()
                    .py_1()
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
                        this.visualizer_zoom = (this.visualizer_zoom + 0.15).clamp(0.5, 3.0);
                        cx.notify();
                    })),
            )
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
            .border_b_1()
            .border_color(theme::glass_highlight())
            .child(hud_summary)
            .child(hud_controls);

        // Selected Quark Focus Card (bottom overlay)
        let selected_node_info = selected_quark.as_ref().and_then(|qid| {
            nodes.iter().find(|n| &n.id == qid).cloned()
        });

        let selected_hud = selected_node_info.map(|node| {
            let qid_for_info = node.id.clone();
            let state_str = if node.is_excited {
                "⚡ Excited (Active Turn / Task)"
            } else if node.is_enabled {
                "● Idle · Ready"
            } else {
                "○ Disabled"
            };

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
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .size_3p5()
                                .rounded_full()
                                .bg(node.color),
                        )
                        .child(
                            v_flex()
                                .gap_0p5()
                                .child(
                                    h_flex()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(theme::text())
                                                .child(node.name),
                                        )
                                        .when(node.is_orchestrator, |this| {
                                             this.child(
                                                div()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(gpui::rgb(0xf59e0b).opacity(0.2))
                                                    .text_xs()
                                                    .text_color(gpui::rgb(0xf59e0b))
                                                    .child("Orchestrator"),
                                            )
                                        }),
                                )
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(format!("{} · {} · {}", node.id, node.model, state_str)),
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

        let sphere_radius = base_radius;
        let nodes_for_canvas = nodes.clone();

        // 3D Canvas Rendering
        let canvas_elem = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _cx| {
                let w = bounds.size.width;
                let h = bounds.size.height;
                let cx_pt = bounds.origin.x + w / 2.0;
                let cy_pt = bounds.origin.y + h / 2.0;
                let cx_f32 = f32::from(cx_pt);
                let cy_f32 = f32::from(cy_pt);
                let distance = 420.0;

                let star_rot = Rotation3D::new(yaw * 0.4, pitch * 0.4);

                // -------------------------------------------------------------
                // 1. Ambient Cosmic Deep-Space Star Dust Field (Precomputed)
                // -------------------------------------------------------------
                for star in STAR_FIELD.iter() {
                    let p = star_rot.transform(star.unit_p.scale(sphere_radius));
                    let (sx, sy, z, scale) = p.project(cx_f32, cy_f32, distance);

                    let star_dist = sphere_radius * star.star_dist_mult;
                    let shimmer = 0.25 + 0.75 * (time_secs * 2.2 + star.seed * 1.4).sin().abs();
                    let depth_a = ((z + star_dist) / (2.0 * star_dist)).clamp(0.15, 0.9);
                    let star_r = px((1.0 + (star.seed % 2.0) * 0.6) * scale);

                    let star_bounds = Bounds {
                        origin: point(px(sx) - star_r, px(sy) - star_r),
                        size: size(star_r * 2.0, star_r * 2.0),
                    };

                    let star_color = match star.color_type {
                        0 => gpui::rgb(0x38bdf8).opacity(0.35 * depth_a * shimmer),
                        1 => gpui::rgb(0x818cf8).opacity(0.28 * depth_a * shimmer),
                        _ => gpui::rgb(0xf8fafc).opacity(0.45 * depth_a * shimmer),
                    };

                    window.paint_quad(fill(star_bounds, star_color).corner_radii(star_r));
                }

                // -------------------------------------------------------------
                // 2. Cosmic Singularity Core (Multi-layered energy nebula)
                // -------------------------------------------------------------
                let core_pulse = 0.85 + 0.15 * (time_secs * 2.6).sin();
                let core_center = point(cx_pt, cy_pt);

                // Layer A: Outer Nebula Glow Haze
                let r_nebula = px(sphere_radius * 0.65 * core_pulse);
                let nebula_bounds = Bounds {
                    origin: point(core_center.x - r_nebula, core_center.y - r_nebula),
                    size: size(r_nebula * 2.0, r_nebula * 2.0),
                };
                window.paint_quad(
                    fill(nebula_bounds, gpui::rgb(0x1e1b4b).opacity(0.16 * core_pulse)).corner_radii(r_nebula),
                );

                // Layer B: Cosmic Energy Corona
                let r_corona = px(sphere_radius * 0.40 * core_pulse);
                let corona_bounds = Bounds {
                    origin: point(core_center.x - r_corona, core_center.y - r_corona),
                    size: size(r_corona * 2.0, r_corona * 2.0),
                };
                window.paint_quad(
                    fill(corona_bounds, gpui::rgb(0x0284c7).opacity(0.20 * core_pulse)).corner_radii(r_corona),
                );

                // Layer C: Inner Quantum Radiant Core
                let r_inner = px(sphere_radius * 0.18 * core_pulse);
                let inner_bounds = Bounds {
                    origin: point(core_center.x - r_inner, core_center.y - r_inner),
                    size: size(r_inner * 2.0, r_inner * 2.0),
                };
                window.paint_quad(
                    fill(inner_bounds, gpui::rgb(0x38bdf8).opacity(0.32 * core_pulse)).corner_radii(r_inner),
                );

                // Layer D: White Singularity Point
                let r_center = px(sphere_radius * 0.06 * core_pulse);
                let center_bounds = Bounds {
                    origin: point(core_center.x - r_center, core_center.y - r_center),
                    size: size(r_center * 2.0, r_center * 2.0),
                };
                window.paint_quad(
                    fill(center_bounds, gpui::rgb(0xf0f9ff).opacity(0.65 * core_pulse)).corner_radii(r_center),
                );

                // -------------------------------------------------------------
                // 3. Fibonacci Constellation Mesh (Precomputed Static Topology)
                // -------------------------------------------------------------
                let mut lattice_projected = [(0.0f32, 0.0f32, 0.0f32, 0.0f32); LATTICE_COUNT];
                for i in 0..LATTICE_COUNT {
                    let p = rot.transform(LATTICE_UNIT_NODES[i].scale(sphere_radius));
                    let (sx, sy, z, scale) = p.project(cx_f32, cy_f32, distance);
                    lattice_projected[i] = (sx, sy, z, scale);
                }

                // Batch front and back lattice wireframe edges into compound paths
                let mut front_builder = PathBuilder::stroke(px(1.0));
                let mut back_builder = PathBuilder::stroke(px(0.75));
                let mut has_front = false;
                let mut has_back = false;

                for &(i, j) in LATTICE_STATIC_EDGES.iter() {
                    let (sx1, sy1, z1, _) = lattice_projected[i];
                    let (sx2, sy2, z2, _) = lattice_projected[j];
                    let avg_z = (z1 + z2) * 0.5;

                    if avg_z > -0.15 * sphere_radius {
                        front_builder.move_to(point(px(sx1), px(sy1)));
                        front_builder.line_to(point(px(sx2), px(sy2)));
                        has_front = true;
                    } else {
                        back_builder.move_to(point(px(sx1), px(sy1)));
                        back_builder.line_to(point(px(sx2), px(sy2)));
                        has_back = true;
                    }
                }

                if has_back {
                    if let Ok(path) = back_builder.build() {
                        window.paint_path(path, gpui::rgb(0x4338ca).opacity(0.06));
                    }
                }
                if has_front {
                    if let Ok(path) = front_builder.build() {
                        window.paint_path(path, gpui::rgb(0x38bdf8).opacity(0.14));
                    }
                }

                // Draw glowing neural data dots on lattice vertices
                for (ix, (sx, sy, z, scale)) in lattice_projected.iter().enumerate() {
                    let depth_factor = ((*z + sphere_radius) / (2.0 * sphere_radius)).clamp(0.1, 1.0);
                    let shimmer = 0.5 + 0.5 * (time_secs * 3.0 + (ix as f32 * 0.8)).sin();
                    let dot_r = px((1.3 + shimmer * 0.8) * scale);
                    let dot_center = point(px(*sx), px(*sy));

                    let dot_bounds = Bounds {
                        origin: point(dot_center.x - dot_r, dot_center.y - dot_r),
                        size: size(dot_r * 2.0, dot_r * 2.0),
                    };

                    if *z > 0.0 {
                        // Front-facing glowing node
                        let halo_r = dot_r * 1.8;
                        let halo_bounds = Bounds {
                            origin: point(dot_center.x - halo_r, dot_center.y - halo_r),
                            size: size(halo_r * 2.0, halo_r * 2.0),
                        };
                        window.paint_quad(
                            fill(halo_bounds, gpui::rgb(0x00f0ff).opacity(0.22 * depth_factor * shimmer))
                                .corner_radii(halo_r),
                        );
                        window.paint_quad(
                            fill(dot_bounds, gpui::rgb(0xbae6fd).opacity(0.80 * depth_factor))
                                .corner_radii(dot_r),
                        );
                    } else {
                        // Back-facing subtle node
                        window.paint_quad(
                            fill(dot_bounds, gpui::rgb(0x6366f1).opacity(0.28 * depth_factor))
                                .corner_radii(dot_r),
                        );
                    }
                }

                // -------------------------------------------------------------
                // 4. Batched Wireframe Latitude Parallels & Longitude Meridians
                // -------------------------------------------------------------
                let mut lat_builder = PathBuilder::stroke(px(0.85));
                for ring in LATITUDE_UNIT_RINGS.iter() {
                    let mut first = true;
                    for &unit_p in ring.iter() {
                        let p = rot.transform(unit_p.scale(sphere_radius));
                        let (sx, sy, _, _) = p.project(cx_f32, cy_f32, distance);
                        if first {
                            lat_builder.move_to(point(px(sx), px(sy)));
                            first = false;
                        } else {
                            lat_builder.line_to(point(px(sx), px(sy)));
                        }
                    }
                }
                if let Ok(path) = lat_builder.build() {
                    window.paint_path(path, gpui::rgb(0x0284c7).opacity(0.11));
                }

                let mut mer_builder = PathBuilder::stroke(px(0.85));
                for meridian in LONGITUDE_UNIT_MERIDIANS.iter() {
                    let mut first = true;
                    for &unit_p in meridian.iter() {
                        let p = rot.transform(unit_p.scale(sphere_radius));
                        let (sx, sy, _, _) = p.project(cx_f32, cy_f32, distance);
                        if first {
                            mer_builder.move_to(point(px(sx), px(sy)));
                            first = false;
                        } else {
                            mer_builder.line_to(point(px(sx), px(sy)));
                        }
                    }
                }
                if let Ok(path) = mer_builder.build() {
                    window.paint_path(path, gpui::rgb(0x0284c7).opacity(0.09));
                }

                // -------------------------------------------------------------
                // 5. Inclined Celestial Equator & Orbiting Quantum Photons
                // -------------------------------------------------------------
                let mut belt_builder = PathBuilder::stroke(px(1.1));
                let mut belt_first = true;
                for &unit_p in BELT_UNIT_RING.iter() {
                    let p = rot.transform(unit_p.scale(sphere_radius));
                    let (sx, sy, _, _) = p.project(cx_f32, cy_f32, distance);
                    if belt_first {
                        belt_builder.move_to(point(px(sx), px(sy)));
                        belt_first = false;
                    } else {
                        belt_builder.line_to(point(px(sx), px(sy)));
                    }
                }
                if let Ok(path) = belt_builder.build() {
                    window.paint_path(path, gpui::rgb(0x00f0ff).opacity(0.18));
                }

                // Traveling quantum photons along the inclined celestial ring
                let belt_radius = sphere_radius * 1.18;
                let belt_tilt = 28.0 * (PI / 180.0);
                for k in 0..16 {
                    let phi = 2.0 * PI * (((k as f32 / 16.0) + time_secs * 0.14) % 1.0);
                    let p = rot.transform(
                        Point3D::from_spherical(belt_radius, 0.0, phi).rotate_z(belt_tilt),
                    );
                    let (sx, sy, z, scale) = p.project(cx_f32, cy_f32, distance);

                    let depth_factor = ((z + belt_radius) / (2.0 * belt_radius)).clamp(0.15, 1.0);
                    let photon_r = px((2.0 + (k % 3) as f32 * 0.8) * scale);
                    let photon_center = point(px(sx), px(sy));

                    let halo_r = photon_r * 2.2;
                    let halo_bounds = Bounds {
                        origin: point(photon_center.x - halo_r, photon_center.y - halo_r),
                        size: size(halo_r * 2.0, halo_r * 2.0),
                    };
                    window.paint_quad(
                        fill(halo_bounds, gpui::rgb(0x00f0ff).opacity(0.25 * depth_factor)).corner_radii(halo_r),
                    );

                    let photon_bounds = Bounds {
                        origin: point(photon_center.x - photon_r, photon_center.y - photon_r),
                        size: size(photon_r * 2.0, photon_r * 2.0),
                    };
                    window.paint_quad(
                        fill(photon_bounds, gpui::rgb(0xe0f2fe).opacity(0.90 * depth_factor)).corner_radii(photon_r),
                    );
                }

                // -------------------------------------------------------------
                // 6. Project and Depth-Sort Swarm Quark Nodes
                // -------------------------------------------------------------
                let mut projected_nodes = nodes_for_canvas.clone();
                for node in &mut projected_nodes {
                    let (sx, sy, z, scale) = node.point_3d.project(cx_f32, cy_f32, distance);
                    node.screen_x = sx;
                    node.screen_y = sy;
                    node.z_depth = z;
                    node.scale = scale;
                }

                // Sort back-to-front for correct alpha blending and rendering
                projected_nodes.sort_by(|a, b| a.z_depth.partial_cmp(&b.z_depth).unwrap_or(std::cmp::Ordering::Equal));

                // -------------------------------------------------------------
                // 7. Active Delegation Streams (In-flight tasks only)
                // -------------------------------------------------------------
                for (from_id, to_id) in &active_tasks {
                    if let (Some(from_node), Some(to_node)) = (
                        projected_nodes.iter().find(|n| &n.id == from_id),
                        projected_nodes.iter().find(|n| &n.id == to_id),
                    ) {
                        let mut builder = PathBuilder::stroke(px(1.8));
                        builder.move_to(point(px(from_node.screen_x), px(from_node.screen_y)));
                        let mid_x = (from_node.screen_x + to_node.screen_x) * 0.5;
                        let mid_y = (from_node.screen_y + to_node.screen_y) * 0.5 - 32.0 * from_node.scale;
                        builder.cubic_bezier_to(
                            point(px(to_node.screen_x), px(to_node.screen_y)),
                            point(px(mid_x), px(mid_y)),
                            point(px(mid_x), px(mid_y)),
                        );

                        if let Ok(path) = builder.build() {
                            let link_color = gpui::rgb(0x06b6d4).opacity(0.55);
                            window.paint_path(path, link_color);
                        }

                        // Traveling energy packet along bezier stream
                        let t_stream = (time_secs * 1.5) % 1.0;
                        let inv_t = 1.0 - t_stream;
                        let pkt_x = inv_t * inv_t * from_node.screen_x + 2.0 * inv_t * t_stream * mid_x + t_stream * t_stream * to_node.screen_x;
                        let pkt_y = inv_t * inv_t * from_node.screen_y + 2.0 * inv_t * t_stream * mid_y + t_stream * t_stream * to_node.screen_y;
                        let pkt_r = px(3.2 * from_node.scale);
                        let pkt_bounds = Bounds {
                            origin: point(px(pkt_x) - pkt_r, px(pkt_y) - pkt_r),
                            size: size(pkt_r * 2.0, pkt_r * 2.0),
                        };
                        window.paint_quad(fill(pkt_bounds, gpui::rgb(0x22c55e)).corner_radii(pkt_r));
                    }
                }

                // -------------------------------------------------------------
                // 8. Draw Depth-Scaled Quark Nodes with Excitation / Crown Halos
                // -------------------------------------------------------------
                for node in &projected_nodes {
                    let radius = px(node.radius * node.scale);
                    let center = point(px(node.screen_x), px(node.screen_y));
                    let bounds = Bounds {
                        origin: point(center.x - radius, center.y - radius),
                        size: size(radius * 2.0, radius * 2.0),
                    };

                    let depth_alpha = ((node.z_depth + sphere_radius) / (2.0 * sphere_radius)).clamp(0.25, 1.0);
                    let is_selected = selected_quark.as_ref().map_or(false, |q| q == &node.id);

                    // Selected Reticle Target Brackets
                    if is_selected {
                        let reticle_r = radius + px(7.0);
                        let reticle_bounds = Bounds {
                            origin: point(center.x - reticle_r, center.y - reticle_r),
                            size: size(reticle_r * 2.0, reticle_r * 2.0),
                        };
                        window.paint_quad(
                            fill(reticle_bounds, gpui::rgb(0x00f0ff).opacity(0.18 * depth_alpha)).corner_radii(px(3.0)),
                        );
                    }

                    // Active Excitation Wave Halos (when actually thinking/working or running in-flight task)
                    if node.is_excited {
                        let pulse1 = ((time_secs * 5.0).sin() * 0.25 + 0.85) as f32;
                        let halo1_r = radius * (1.9 * pulse1);
                        let halo1_bounds = Bounds {
                            origin: point(center.x - halo1_r, center.y - halo1_r),
                            size: size(halo1_r * 2.0, halo1_r * 2.0),
                        };
                        window.paint_quad(
                            fill(halo1_bounds, gpui::rgb(0x22c55e).opacity(0.28 * depth_alpha)).corner_radii(halo1_r),
                        );

                        let pulse2 = (((time_secs * 5.0) - 1.2).sin() * 0.25 + 0.85) as f32;
                        let halo2_r = radius * (2.7 * pulse2);
                        let halo2_bounds = Bounds {
                            origin: point(center.x - halo2_r, center.y - halo2_r),
                            size: size(halo2_r * 2.0, halo2_r * 2.0),
                        };
                        window.paint_quad(
                            fill(halo2_bounds, gpui::rgb(0x22c55e).opacity(0.14 * depth_alpha)).corner_radii(halo2_r),
                        );
                    }

                    // Orchestrator Golden Crown Halo
                    if node.is_orchestrator {
                        let crown_r = radius + px(4.0);
                        let crown_bounds = Bounds {
                            origin: point(center.x - crown_r, center.y - crown_r),
                            size: size(crown_r * 2.0, crown_r * 2.0),
                        };
                        window.paint_quad(
                            fill(crown_bounds, gpui::rgb(0xf59e0b).opacity(0.40 * depth_alpha)).corner_radii(crown_r),
                        );
                    }

                    // Outer Dark Obsidian Glass Rim
                    let border_r = radius + px(1.8);
                    let border_bounds = Bounds {
                        origin: point(center.x - border_r, center.y - border_r),
                        size: size(border_r * 2.0, border_r * 2.0),
                    };
                    window.paint_quad(
                        fill(border_bounds, theme::canvas_base()).corner_radii(border_r),
                    );

                    // Core Quark Node Disk
                    let node_color = if node.is_enabled {
                        node.color.opacity(depth_alpha)
                    } else {
                        gpui::rgb(0x64748b).opacity(depth_alpha * 0.4).into()
                    };
                    window.paint_quad(fill(bounds, node_color).corner_radii(radius));

                    // Specular Center Pinpoint
                    let spec_r = radius * 0.35;
                    let spec_bounds = Bounds {
                        origin: point(center.x - spec_r * 0.7, center.y - spec_r * 0.7),
                        size: size(spec_r * 1.4, spec_r * 1.4),
                    };
                    window.paint_quad(
                        fill(spec_bounds, gpui::rgb(0xffffff).opacity(0.65 * depth_alpha)).corner_radii(spec_r),
                    );
                }
            },
        )
        .size_full();

        // Canvas container with mouse drag rotation, scroll zoom, and click-to-select
        let canvas_container = div()
            .relative()
            .flex_1()
            .min_h_0()
            .cursor_grab()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _window, cx| {
                    this.visualizer_last_mouse = Some(ev.position);
                    this.visualizer_auto_spin = false;

                    // Hit test for clicked quark node
                    let click_x = f32::from(ev.position.x);
                    let click_y = f32::from(ev.position.y);

                    let mut clicked_quark: Option<String> = None;
                    let mut min_dist_sq = 24.0 * 24.0; // 24px hit threshold

                    for node in &nodes {
                        let dx = node.screen_x - click_x;
                        let dy = node.screen_y - click_y;
                        let d_sq = dx * dx + dy * dy;
                        if d_sq < min_dist_sq {
                            min_dist_sq = d_sq;
                            clicked_quark = Some(node.id.clone());
                        }
                    }

                    if let Some(qid) = clicked_quark {
                        this.visualizer_selected_quark = Some(qid);
                    }
                    cx.notify();
                }),
            )
            .on_mouse_up(
                MouseButton::Left,
                cx.listener(|this, _ev: &gpui::MouseUpEvent, _window, cx| {
                    this.visualizer_last_mouse = None;
                    cx.notify();
                }),
            )
            .on_mouse_move(cx.listener(|this, ev: &gpui::MouseMoveEvent, _window, cx| {
                if ev.pressed_button == Some(MouseButton::Left) {
                    if let Some(last_pos) = this.visualizer_last_mouse {
                        let dx = f32::from(ev.position.x - last_pos.x);
                        let dy = f32::from(ev.position.y - last_pos.y);
                        this.visualizer_yaw += dx * 0.008;
                        this.visualizer_pitch = (this.visualizer_pitch - dy * 0.008).clamp(-1.45, 1.45);
                        this.visualizer_last_mouse = Some(ev.position);
                        cx.notify();
                    } else {
                        this.visualizer_last_mouse = Some(ev.position);
                    }
                } else {
                    this.visualizer_last_mouse = None;
                }
            }))
            .on_scroll_wheel(cx.listener(|this, ev: &gpui::ScrollWheelEvent, _window, cx| {
                let delta = match ev.delta {
                    gpui::ScrollDelta::Lines(d) => d.y * 0.1,
                    gpui::ScrollDelta::Pixels(d) => f32::from(d.y) * 0.002,
                };
                this.visualizer_zoom = (this.visualizer_zoom + delta).clamp(0.5, 3.0);
                cx.notify();
            }))
            .child(canvas_elem);

        v_flex()
            .id("visualizer-container")
            .size_full()
            .bg(theme::canvas_base())
            .child(header)
            .child(canvas_container)
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
    fn rotation3d_matches_point3d_rotate() {
        let test_points = [
            Point3D::new(50.0, 30.0, 40.0),
            Point3D::new(-120.0, 80.0, -35.0),
            Point3D::new(0.0, 100.0, 0.0),
            Point3D::new(10.0, -20.0, 70.0),
        ];
        let angles = [(0.0, 0.0), (0.75, -0.4), (1.2, 0.8), (-0.9, -1.1), (PI, -PI / 3.0)];

        for &(yaw, pitch) in &angles {
            let rot = Rotation3D::new(yaw, pitch);
            for &p in &test_points {
                let p_rot = p.rotate(yaw, pitch);
                let p_mat = rot.transform(p);
                assert!(
                    (p_rot.x - p_mat.x).abs() < 1e-4
                        && (p_rot.y - p_mat.y).abs() < 1e-4
                        && (p_rot.z - p_mat.z).abs() < 1e-4,
                    "Rotation3D::transform must match Point3D::rotate exactly for yaw={yaw}, pitch={pitch}"
                );
            }
        }
    }

    #[test]
    fn point3d_rotate_z_preserves_radius() {
        let p = Point3D::new(40.0, -20.0, 30.0);
        let orig_dist = (p.x * p.x + p.y * p.y + p.z * p.z).sqrt();

        let rotated = p.rotate_z(0.65);
        let rot_dist = (rotated.x * rotated.x + rotated.y * rotated.y + rotated.z * rotated.z).sqrt();

        assert!((orig_dist - rot_dist).abs() < 1e-3, "Roll rotation around Z must preserve Euclidean radius");
    }

    #[test]
    fn point3d_perspective_projection_scales_with_depth() {
        let p_front = Point3D::new(10.0, 20.0, 50.0);
        let p_back = Point3D::new(10.0, 20.0, -50.0);

        let (_, _, _, scale_front) = p_front.project(200.0, 200.0, 400.0);
        let (_, _, _, scale_back) = p_back.project(200.0, 200.0, 400.0);

        assert!(scale_front > scale_back, "Front point must have larger perspective scale than back point");
    }

    #[test]
    fn point3d_dist_sq_matches_euclidean() {
        let p1 = Point3D::new(0.0, 0.0, 0.0);
        let p2 = Point3D::new(3.0, 4.0, 0.0);
        assert!((p1.dist_sq(&p2) - 25.0).abs() < 1e-4);
    }

    #[test]
    fn precomputed_lattice_topology_invariants() {
        assert_eq!(LATTICE_UNIT_NODES.len(), 64);
        assert!(!LATTICE_STATIC_EDGES.is_empty());
        // Verify all edge indices are valid
        for &(i, j) in LATTICE_STATIC_EDGES.iter() {
            assert!(i < 64 && j < 64);
            assert!(i < j);
            let dist_sq = LATTICE_UNIT_NODES[i].dist_sq(&LATTICE_UNIT_NODES[j]);
            assert!(dist_sq < 0.44 * 0.44 + 1e-5);
        }
    }
}
