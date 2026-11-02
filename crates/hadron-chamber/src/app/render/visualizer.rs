//! Interactive 3D Sphere Topology visualizer for Hadron swarm.
//!
//! Projects quarks and their active execution/spawning topology onto a
//! mathematical 3D cosmic sphere using perspective projection, depth scaling,
//! neural constellation wireframe lattices, glowing singularity core,
//! traveling orbital photon belt, animated excitation halos, and dynamic delegation links.

use std::collections::{BTreeMap, HashMap, HashSet};
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
    #[cfg(test)]
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
    #[cfg(test)]
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

/// Resolve the phase-specific excitation halo color for a quark's live activity.
pub fn phase_halo_color(doing: Option<hadron_lattice::live::Doing>) -> Hsla {
    match doing {
        Some(hadron_lattice::live::Doing::Thinking) => theme::halo_reasoning(), // Warm Violet #A87FFB
        Some(hadron_lattice::live::Doing::Working) => theme::success().into(), // Emerald-Mint #62C073
        Some(hadron_lattice::live::Doing::Planning) => theme::halo_reasoning(),
        Some(hadron_lattice::live::Doing::Speaking) => theme::warning().into(), // Warm Amber #E5C07B
        Some(hadron_lattice::live::Doing::Gating) => theme::info().into(), // Muted Orchid #C778DD
        None => theme::success().into(),
    }
}

/// Compute (yaw, pitch) camera rotation angles to bring a node at spherical coordinates
/// (theta, phi) directly to the front-facing center of the view (x = 0, y = 0, z > 0).
pub fn focus_angles_for_spherical(theta: f32, phi: f32) -> (f32, f32) {
    let mut yaw = (phi - PI * 0.5) % (2.0 * PI);
    if yaw < 0.0 {
        yaw += 2.0 * PI;
    }
    let pitch = theta.clamp(-1.45, 1.45);
    (yaw, pitch)
}

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
    pub live_phase: Option<hadron_lattice::live::Doing>,
    pub live_detail: Option<String>,
    pub task_title: Option<String>,
    pub elapsed_secs: Option<u64>,
    pub tokens: u64,
    pub is_seated: bool,
    pub theta: f32,
    pub phi: f32,
    pub point_3d: Point3D,
    pub screen_x: f32,
    pub screen_y: f32,
    pub z_depth: f32,
    pub scale: f32,
    pub radius: f32,
}

impl super::Chamber {
    pub(crate) fn has_excited_visualizer_nodes(&self) -> bool {
        self.view.roster.iter().any(|row| {
            let activity = self
                .last_live_activities
                .get(&row.id)
                .and_then(|a| a.as_ref());
            let has_active_task = self.view.tasks.iter().any(|t| {
                (t.to == row.id || t.from == row.id) && t.state == TaskState::Working
            });
            activity.is_some() || has_active_task
        })
    }

    pub(crate) fn pump_visualizer(&mut self, cx: &mut Context<Self>) {
        if self.workspace_mode == WorkspaceMode::Selector
            || self.prefs.inspector_collapsed
            || self.right_rail_tab != RightRailTab::Visualizer
        {
            self.visualizer_last_tick = None;
            return;
        }
        let now = std::time::Instant::now();
        let dt = self
            .visualizer_last_tick
            .map(|prev| now.saturating_duration_since(prev).as_secs_f32().min(0.1))
            .unwrap_or(0.033);
        self.visualizer_last_tick = Some(now);

        if self.visualizer_auto_spin {
            self.visualizer_yaw = (self.visualizer_yaw + dt * 0.28) % (2.0 * PI);
            cx.notify();
        } else if self.has_excited_visualizer_nodes() {
            cx.notify();
        }
    }

    /// Render the interactive 3D Cosmic Sphere Topology visualizer view.
    pub(super) fn swarm_3d_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let now = chrono::Utc::now();
        let time_secs = (now.timestamp_millis() % 1_000_000) as f32 / 1000.0;

        let yaw = self.visualizer_yaw;
        let pitch = self.visualizer_pitch;

        // Precompute rotation matrices for the frame
        let rot = Rotation3D::new(yaw, pitch);

        let roster = &self.view.roster;
        let total_quarks = roster.len();

        // Calculate active and excited counts accurately:
        // A quark is excited ONLY if it currently has fresh activity (thinking/working/planning/speaking)
        // OR an in-flight working task (TaskState::Working). Completed/Done tasks do NOT excite.
        let mut excited_quark_ids = std::collections::HashSet::new();
        for row in roster.iter() {
            let activity = self.last_live_activities.get(&row.id).and_then(|a| a.as_ref());
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

        // Ingest rich live activity capsules and active tasks
        let rich_capsules = crate::app::widgets::active_quarks_rich_with_tasks(
            roster,
            &self.view.tasks,
            |id| self.last_live_activities.get(id).and_then(|a| a.clone()),
            now,
        );
        let mut rich_by_id = std::collections::HashMap::new();
        for cap in rich_capsules {
            rich_by_id.insert(cap.quark_id.clone(), cap);
        }

        let active_task_for_row = |row_id: &str| -> Option<String> {
            self.view.tasks.iter().find(|t| (t.to == row_id || t.from == row_id) && t.state == TaskState::Working).map(|t| t.title.clone())
        };

        // Dual-shell spherical distribution:
        // Inner shell (1.0 * base_radius): seated, active swarm quarks
        // Outer shell (1.28 * base_radius): catalogue / unadopted quarks
        let mut seated_quarks: Vec<&RosterRow> = Vec::new();
        let mut catalogue_quarks: Vec<&RosterRow> = Vec::new();
        for row in roster.iter() {
            if row.enabled && row.adopted {
                seated_quarks.push(row);
            } else {
                catalogue_quarks.push(row);
            }
        }

        let seated_has_orch = seated_quarks.iter().any(|r| matches!(r.flavor, Some(hadron_lattice::Flavor::Orchestrator)));
        let non_orch_seated_count = seated_quarks.len().saturating_sub(if seated_has_orch { 1 } else { 0 }).max(1);
        let cat_count = catalogue_quarks.len().max(1);

        let mut nodes: Vec<ProjectedQuarkNode> = Vec::with_capacity(total_quarks);
        let mut seated_worker_idx = 0;

        for row in seated_quarks.iter() {
            let is_orchestrator = matches!(row.flavor, Some(hadron_lattice::Flavor::Orchestrator));
            let color = self.color_for(&row.id);
            let resolved = self.resolve_identity(&row.id);
            let is_excited = excited_quark_ids.contains(&row.id);
            let cap_opt = rich_by_id.get(&row.id);
            let live_phase = cap_opt.map(|c| c.doing);
            let live_detail = cap_opt.map(|c| c.detail.clone());
            let elapsed_secs = cap_opt.and_then(|c| c.elapsed_secs);
            let task_title = active_task_for_row(&row.id);

            let (theta, phi) = if is_orchestrator {
                (1.12f32, 0.0f32)
            } else {
                let lat_frac = ((seated_worker_idx as f32 + 0.5) / non_orch_seated_count as f32) * 2.0 - 1.0;
                let th = (lat_frac * 0.75).asin();
                let ph = seated_worker_idx as f32 * 2.39996323;
                seated_worker_idx += 1;
                (th, ph)
            };

            let p3d = Point3D::from_spherical(base_radius * 1.0, theta, phi);
            let rotated = rot.transform(p3d);

            nodes.push(ProjectedQuarkNode {
                id: row.id.clone(),
                name: resolved.name,
                model: if row.model.is_empty() { "default".into() } else { row.model.clone() },
                color,
                is_orchestrator,
                is_enabled: true,
                is_excited,
                live_phase,
                live_detail,
                task_title,
                elapsed_secs,
                tokens: row.tokens as u64,
                is_seated: true,
                theta,
                phi,
                point_3d: rotated,
                screen_x: 0.0,
                screen_y: 0.0,
                z_depth: rotated.z,
                scale: 1.0,
                radius: if is_orchestrator { 9.5 } else { 7.0 },
            });
        }

        for (c_idx, row) in catalogue_quarks.iter().enumerate() {
            let is_orchestrator = matches!(row.flavor, Some(hadron_lattice::Flavor::Orchestrator));
            let color = self.color_for(&row.id);
            let resolved = self.resolve_identity(&row.id);
            let is_excited = excited_quark_ids.contains(&row.id);
            let cap_opt = rich_by_id.get(&row.id);
            let live_phase = cap_opt.map(|c| c.doing);
            let live_detail = cap_opt.map(|c| c.detail.clone());
            let elapsed_secs = cap_opt.and_then(|c| c.elapsed_secs);
            let task_title = active_task_for_row(&row.id);

            let lat_frac = ((c_idx as f32 + 0.5) / cat_count as f32) * 2.0 - 1.0;
            let theta = (lat_frac * 0.85).asin();
            let phi = c_idx as f32 * 2.39996323 + 1.25;

            let p3d = Point3D::from_spherical(base_radius * 1.28, theta, phi);
            let rotated = rot.transform(p3d);

            nodes.push(ProjectedQuarkNode {
                id: row.id.clone(),
                name: resolved.name,
                model: if row.model.is_empty() { "default".into() } else { row.model.clone() },
                color,
                is_orchestrator,
                is_enabled: row.enabled && row.adopted,
                is_excited,
                live_phase,
                live_detail,
                task_title,
                elapsed_secs,
                tokens: row.tokens as u64,
                is_seated: false,
                theta,
                phi,
                point_3d: rotated,
                screen_x: 0.0,
                screen_y: 0.0,
                z_depth: rotated.z,
                scale: 1.0,
                radius: 5.5,
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
                        .bg(theme::success().opacity(0.18))
                        .border_1()
                        .border_color(theme::success().opacity(0.45))
                        .text_xs()
                        .font_weight(gpui::FontWeight::BOLD)
                        .text_color(theme::success())
                        .child(
                            div()
                                .size_2()
                                .rounded_full()
                                .bg(theme::success()),
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
                                .bg(theme::info().opacity(0.4)),
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
                        this.visualizer_last_tick = Some(std::time::Instant::now());
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

        let mode_switcher = self.visualizer_mode_switcher_header(cx);

        // Header strip
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
            .child(hud_summary)
            .child(
                h_flex()
                    .w_full()
                    .justify_end()
                    .items_center()
                    .child(hud_controls),
            );

        // Interactive Live Swarm Activity & Seated Quarks Dock
        let seated_nodes: Vec<ProjectedQuarkNode> = nodes.iter().filter(|n| n.is_seated).cloned().collect();
        let current_selected = selected_quark.clone();
        let activity_dock = if seated_nodes.is_empty() {
            None
        } else {
            Some(
                h_flex()
                    .id("vis-activity-dock")
                    .w_full()
                    .min_w_0()
                    .overflow_hidden()
                    .flex_wrap()
                    .px_3()
                    .py_2()
                    .gap_1p5()
                    .border_b_1()
                    .border_color(theme::glass_highlight())
                    .bg(theme::bg_surface().opacity(0.40))
                    .children(seated_nodes.iter().map(|node| {
                        let qid = node.id.clone();
                        let th = node.theta;
                        let ph = node.phi;
                        let is_sel = current_selected.as_deref() == Some(&node.id);
                        let p_color: Hsla = phase_halo_color(node.live_phase);
                        let p_rgba: gpui::Rgba = p_color.into();

                        let chip_bg = if is_sel {
                            theme::tab_bar_bg()
                        } else if node.is_excited {
                            p_rgba.opacity(0.12)
                        } else {
                            theme::tab_bar_bg().opacity(0.60)
                        };

                        let chip_border = if is_sel {
                            theme::accent()
                        } else if node.is_excited {
                            p_rgba.opacity(0.40)
                        } else {
                            theme::glass_highlight().into()
                        };

                        h_flex()
                            .id(SharedString::from(format!("vis-dock-chip-{}", node.id)))
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
                                    .flex_shrink_0()
                                    .size_2()
                                    .rounded_full()
                                    .bg(node.color),
                            )
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::MEDIUM)
                                    .text_color(if is_sel { theme::text() } else { theme::text_secondary() })
                                    .truncate()
                                    .max_w(px(110.0))
                                    .child(node.name.clone()),
                            )
                            .when(node.is_excited, |chip| {
                                let phase_label = match node.live_phase {
                                    Some(hadron_lattice::live::Doing::Thinking) => "thinking",
                                    Some(hadron_lattice::live::Doing::Working) => "exec",
                                    Some(hadron_lattice::live::Doing::Planning) => "planning",
                                    Some(hadron_lattice::live::Doing::Speaking) => "speaking",
                                    Some(hadron_lattice::live::Doing::Gating) => "gating",
                                    None => "active",
                                };
                                chip.child(
                                    div()
                                        .flex_shrink_0()
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(p_color.opacity(0.18))
                                        .text_xs()
                                        .font_weight(gpui::FontWeight::BOLD)
                                        .text_color(p_color)
                                        .child(phase_label),
                                )
                            })
                            .when_some(node.elapsed_secs.filter(|_| node.is_excited), |chip, secs| {
                                chip.child(
                                    div()
                                        .flex_shrink_0()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(super::chat::format_duration(secs as i64)),
                                )
                            })
                            .hover(|s| s.border_color(theme::accent()).bg(theme::tab_bar_bg()))
                            .on_click(cx.listener(move |this, _, _, cx| {
                                this.visualizer_selected_quark = Some(qid.clone());
                                let (snap_yaw, snap_pitch) = focus_angles_for_spherical(th, ph);
                                this.visualizer_yaw = snap_yaw;
                                this.visualizer_pitch = snap_pitch;
                                this.visualizer_auto_spin = false;
                                cx.notify();
                            }))
                    }))
            )
        };

        // Selected Quark Focus Card (bottom overlay)
        let selected_node_info = selected_quark.as_ref().and_then(|qid| {
            nodes.iter().find(|n| &n.id == qid).cloned()
        });

        let selected_hud = selected_node_info.map(|node| {
            let qid_for_info = node.id.clone();
            let state_str = if node.is_excited {
                match node.live_phase {
                    Some(hadron_lattice::live::Doing::Thinking) => "⚡ Thinking (Reasoning)",
                    Some(hadron_lattice::live::Doing::Working) => "⚡ Working (Tool Execution)",
                    Some(hadron_lattice::live::Doing::Planning) => "⚡ Planning (Task Breakdown)",
                    Some(hadron_lattice::live::Doing::Speaking) => "⚡ Speaking (Field Reply)",
                    Some(hadron_lattice::live::Doing::Gating) => "⚡ Gating (Merge Gate / Checks)",
                    None => "⚡ Excited (Active Turn / Task)",
                }
            } else if node.is_enabled {
                "● Idle · Ready"
            } else {
                "○ Catalogue · Inactive"
            };

            let ctx_opt = self.latest_context(&node.id);
            let telemetry_str = if let Some(ctx) = ctx_opt {
                format!(
                    "Context: {} / {} ({:.0}%) · {} tokens",
                    format_num(ctx.used_tokens as u64),
                    format_num(ctx.context_window_size as u64),
                    ctx.used_percentage,
                    format_num(node.tokens),
                )
            } else if node.tokens > 0 {
                format!("Spend: {} tokens", format_num(node.tokens))
            } else {
                "Spend: 0 tokens".to_string()
            };

            let topology_tier = if node.is_seated {
                "Primary Swarm (Inner 1.0x)"
            } else {
                "Swarm Catalogue (Outer 1.28x)"
            };

            let phase_color = phase_halo_color(node.live_phase);
            let phase_rgba: gpui::Rgba = phase_color.into();

            h_flex()
                .w_full()
                .min_w_0()
                .overflow_hidden()
                .gap_2()
                .justify_between()
                .items_center()
                .p_3()
                .rounded_lg()
                .bg(theme::term_bg())
                .border_1()
                .border_color(theme::glass_highlight())
                .child(
                    h_flex()
                        .flex_1()
                        .min_w_0()
                        .overflow_hidden()
                        .gap_3()
                        .items_center()
                        .child(
                            div()
                                .flex_shrink_0()
                                .size_4()
                                .rounded_full()
                                .bg(node.color)
                                .border_2()
                                .border_color(if node.is_excited { phase_rgba } else { theme::border() }),
                        )
                        .child(
                            v_flex()
                                .flex_1()
                                .min_w_0()
                                .overflow_hidden()
                                .gap_0p5()
                                .child(
                                    h_flex()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .gap_2()
                                        .items_center()
                                        .child(
                                            div()
                                                .text_sm()
                                                .font_weight(gpui::FontWeight::BOLD)
                                                .text_color(theme::text())
                                                .truncate()
                                                .child(node.name),
                                        )
                                        .when(node.is_orchestrator, |this| {
                                             this.child(
                                                div()
                                                    .flex_shrink_0()
                                                    .px_1p5()
                                                    .py_0p5()
                                                    .rounded_sm()
                                                    .bg(theme::warning().opacity(0.2))
                                                    .text_xs()
                                                    .text_color(theme::warning())
                                                    .child("Orchestrator"),
                                            )
                                        })
                                        .child(
                                            div()
                                                .flex_shrink_0()
                                                .px_1p5()
                                                .py_0p5()
                                                .rounded_sm()
                                                .bg(if node.is_excited { phase_rgba.opacity(0.18) } else { theme::tab_bar_bg() })
                                                .border_1()
                                                .border_color(if node.is_excited { phase_rgba.opacity(0.4) } else { theme::glass_highlight().into() })
                                                .text_xs()
                                                .font_weight(gpui::FontWeight::MEDIUM)
                                                .text_color(if node.is_excited { phase_rgba } else { theme::text_muted() })
                                                .child(state_str),
                                        ),
                                )
                                .child(
                                    h_flex()
                                        .min_w_0()
                                        .overflow_hidden()
                                        .gap_2()
                                        .items_center()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(
                                            div()
                                                .truncate()
                                                .child(format!("{} · {} · {} · {}", node.id, node.model, topology_tier, telemetry_str)),
                                        ),
                                )
                                .when_some(node.live_detail, |this, detail| {
                                    this.child(
                                        h_flex()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .gap_1p5()
                                            .items_center()
                                            .text_xs()
                                            .child(div().flex_shrink_0().font_weight(gpui::FontWeight::SEMIBOLD).text_color(phase_color).child("Action:"))
                                            .child(div().truncate().text_color(theme::text()).child(detail)),
                                    )
                                })
                                .when_some(node.task_title, |this, task| {
                                    this.child(
                                        h_flex()
                                            .min_w_0()
                                            .overflow_hidden()
                                            .gap_1p5()
                                            .items_center()
                                            .text_xs()
                                            .child(div().flex_shrink_0().font_weight(gpui::FontWeight::SEMIBOLD).text_color(theme::accent()).child("Task:"))
                                            .child(div().truncate().text_color(theme::text_secondary()).child(task)),
                                    )
                                }),
                        ),
                )
                .child(
                    h_flex()
                        .flex_shrink_0()
                        .gap_2()
                        .child(
                            text_button("vis-open-info", "View Details").on_click(cx.listener(
                                move |this, _, _window, cx| {
                                    this.open_quark_info(qid_for_info.clone(), InfoTab::Home, cx);
                                },
                            )),
                        ),
                )
        });

        let sphere_radius = base_radius;
        let nodes_for_canvas = nodes.clone();
        let hit_nodes_for_canvas = self.visualizer_hit_nodes.clone();

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
                        0 => gpui::rgb(0xa87ffb).opacity(0.35 * depth_a * shimmer),
                        1 => gpui::rgb(0xc778dd).opacity(0.28 * depth_a * shimmer),
                        _ => gpui::rgb(0xededed).opacity(0.45 * depth_a * shimmer),
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
                    fill(nebula_bounds, gpui::rgb(0x262626).opacity(0.22 * core_pulse)).corner_radii(r_nebula),
                );

                // Layer B: Cosmic Energy Corona
                let r_corona = px(sphere_radius * 0.40 * core_pulse);
                let corona_bounds = Bounds {
                    origin: point(core_center.x - r_corona, core_center.y - r_corona),
                    size: size(r_corona * 2.0, r_corona * 2.0),
                };
                window.paint_quad(
                    fill(corona_bounds, gpui::rgb(0xa87ffb).opacity(0.18 * core_pulse)).corner_radii(r_corona),
                );

                // Layer C: Inner Quantum Radiant Core
                let r_inner = px(sphere_radius * 0.18 * core_pulse);
                let inner_bounds = Bounds {
                    origin: point(core_center.x - r_inner, core_center.y - r_inner),
                    size: size(r_inner * 2.0, r_inner * 2.0),
                };
                window.paint_quad(
                    fill(inner_bounds, gpui::rgb(0xc778dd).opacity(0.32 * core_pulse)).corner_radii(r_inner),
                );

                // Layer D: White Singularity Point
                let r_center = px(sphere_radius * 0.06 * core_pulse);
                let center_bounds = Bounds {
                    origin: point(core_center.x - r_center, core_center.y - r_center),
                    size: size(r_center * 2.0, r_center * 2.0),
                };
                window.paint_quad(
                    fill(center_bounds, gpui::rgb(0xededed).opacity(0.65 * core_pulse)).corner_radii(r_center),
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
                        window.paint_path(path, gpui::rgb(0xa87ffb).opacity(0.06));
                    }
                }
                if has_front {
                    if let Ok(path) = front_builder.build() {
                        window.paint_path(path, gpui::rgb(0xc778dd).opacity(0.14));
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
                            fill(halo_bounds, gpui::rgb(0xa87ffb).opacity(0.22 * depth_factor * shimmer))
                                .corner_radii(halo_r),
                        );
                        window.paint_quad(
                            fill(dot_bounds, gpui::rgb(0xededed).opacity(0.80 * depth_factor))
                                .corner_radii(dot_r),
                        );
                    } else {
                        // Back-facing subtle node
                        window.paint_quad(
                            fill(dot_bounds, gpui::rgb(0x8e8e8e).opacity(0.28 * depth_factor))
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
                    window.paint_path(path, gpui::rgb(0xa87ffb).opacity(0.11));
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
                    window.paint_path(path, gpui::rgb(0xa87ffb).opacity(0.09));
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
                    window.paint_path(path, gpui::rgb(0xc778dd).opacity(0.18));
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
                        fill(halo_bounds, gpui::rgb(0xa87ffb).opacity(0.25 * depth_factor)).corner_radii(halo_r),
                    );

                    let photon_bounds = Bounds {
                        origin: point(photon_center.x - photon_r, photon_center.y - photon_r),
                        size: size(photon_r * 2.0, photon_r * 2.0),
                    };
                    window.paint_quad(
                        fill(photon_bounds, gpui::rgb(0xededed).opacity(0.90 * depth_factor)).corner_radii(photon_r),
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

                if let Ok(mut hit_nodes) = hit_nodes_for_canvas.try_borrow_mut() {
                    hit_nodes.clear();
                    for node in &projected_nodes {
                        hit_nodes.push((node.id.clone(), node.screen_x, node.screen_y));
                    }
                }

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
                            let link_color = gpui::rgb(0xa87ffb).opacity(0.55);
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
                        window.paint_quad(fill(pkt_bounds, theme::success()).corner_radii(pkt_r));
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

                    let max_dist = sphere_radius * if node.is_seated { 1.0 } else { 1.28 };
                    let depth_alpha = ((node.z_depth + max_dist) / (2.0 * max_dist)).clamp(0.20, 1.0);
                    let is_selected = selected_quark.as_ref().map_or(false, |q| q == &node.id);

                    // Selected Reticle Target Brackets
                    if is_selected {
                        let reticle_r = radius + px(7.0);
                        let reticle_bounds = Bounds {
                            origin: point(center.x - reticle_r, center.y - reticle_r),
                            size: size(reticle_r * 2.0, reticle_r * 2.0),
                        };
                        window.paint_quad(
                            fill(reticle_bounds, theme::info().opacity(0.22 * depth_alpha)).corner_radii(px(3.0)),
                        );
                    }

                    // Active Excitation Wave Halos & Phase Special Effects
                    if node.is_excited {
                        let halo_color = phase_halo_color(node.live_phase);

                        let pulse1 = ((time_secs * 5.0).sin() * 0.25 + 0.85) as f32;
                        let halo1_r = radius * (1.9 * pulse1);
                        let halo1_bounds = Bounds {
                            origin: point(center.x - halo1_r, center.y - halo1_r),
                            size: size(halo1_r * 2.0, halo1_r * 2.0),
                        };
                        window.paint_quad(
                            fill(halo1_bounds, halo_color.opacity(0.30 * depth_alpha)).corner_radii(halo1_r),
                        );

                        let pulse2 = (((time_secs * 5.0) - 1.2).sin() * 0.25 + 0.85) as f32;
                        let halo2_r = radius * (2.7 * pulse2);
                        let halo2_bounds = Bounds {
                            origin: point(center.x - halo2_r, center.y - halo2_r),
                            size: size(halo2_r * 2.0, halo2_r * 2.0),
                        };
                        window.paint_quad(
                            fill(halo2_bounds, halo_color.opacity(0.15 * depth_alpha)).corner_radii(halo2_r),
                        );

                        // Phase-specific effects
                        match node.live_phase {
                            Some(hadron_lattice::live::Doing::Working) => {
                                // Orbiting tool satellite particle
                                let sat_angle = (time_secs * 4.5) % (2.0 * PI);
                                let sat_dist = radius + px(11.0 * node.scale);
                                let sat_dist_f = f32::from(sat_dist);
                                let sat_x = center.x + px(sat_dist_f * sat_angle.cos());
                                let sat_y = center.y + px(sat_dist_f * sat_angle.sin());
                                let sat_r = px(2.2 * node.scale);
                                let sat_bounds = Bounds {
                                    origin: point(sat_x - sat_r, sat_y - sat_r),
                                    size: size(sat_r * 2.0, sat_r * 2.0),
                                };
                                window.paint_quad(
                                    fill(sat_bounds, theme::info().opacity(0.95 * depth_alpha)).corner_radii(sat_r),
                                );
                                let sat_aura_r = sat_r * 1.8;
                                let sat_aura_bounds = Bounds {
                                    origin: point(sat_x - sat_aura_r, sat_y - sat_aura_r),
                                    size: size(sat_aura_r * 2.0, sat_aura_r * 2.0),
                                };
                                window.paint_quad(
                                    fill(sat_aura_bounds, theme::info().opacity(0.35 * depth_alpha)).corner_radii(sat_aura_r),
                                );
                            }
                            Some(hadron_lattice::live::Doing::Gating) => {
                                // Cyan energy beam to central singularity core
                                let mut beam = PathBuilder::stroke(px(2.0 * node.scale));
                                beam.move_to(center);
                                beam.line_to(point(cx_pt, cy_pt));
                                if let Ok(path) = beam.build() {
                                    window.paint_path(path, theme::info().opacity(0.65 * depth_alpha));
                                }
                            }
                            Some(hadron_lattice::live::Doing::Speaking) => {
                                // Amber broadcast wave expansion
                                let speak_pulse = ((time_secs * 3.5).sin() * 0.35 + 0.85) as f32;
                                let speak_r = radius * (3.5 * speak_pulse);
                                let speak_bounds = Bounds {
                                    origin: point(center.x - speak_r, center.y - speak_r),
                                    size: size(speak_r * 2.0, speak_r * 2.0),
                                };
                                window.paint_quad(
                                    fill(speak_bounds, theme::warning().opacity(0.12 * depth_alpha)).corner_radii(speak_r),
                                );
                            }
                            _ => {}
                        }
                    }

                    // Orchestrator Golden Crown Halo
                    if node.is_orchestrator {
                        let crown_r = radius + px(4.0);
                        let crown_bounds = Bounds {
                            origin: point(center.x - crown_r, center.y - crown_r),
                            size: size(crown_r * 2.0, crown_r * 2.0),
                        };
                        window.paint_quad(
                            fill(crown_bounds, theme::warning().opacity(0.40 * depth_alpha)).corner_radii(crown_r),
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

                    // Core Quark Node Disk (dimmer for catalogue shell)
                    let node_color = if node.is_seated {
                        node.color.opacity(depth_alpha)
                    } else {
                        node.color.opacity(depth_alpha * 0.45)
                    };
                    window.paint_quad(fill(bounds, node_color).corner_radii(radius));

                    // Specular Center Pinpoint
                    if node.is_seated {
                        let spec_r = radius * 0.35;
                        let spec_bounds = Bounds {
                            origin: point(center.x - spec_r * 0.7, center.y - spec_r * 0.7),
                            size: size(spec_r * 1.4, spec_r * 1.4),
                        };
                        window.paint_quad(
                            fill(spec_bounds, gpui::rgb(0xffffff).opacity(0.65 * depth_alpha)).corner_radii(spec_r),
                        );
                    }
                }
            },
        )
        .size_full();

        // Node angles map for canvas click-to-snap rotation
        let node_angles_map: std::collections::HashMap<String, (f32, f32)> = nodes
            .iter()
            .map(|n| (n.id.clone(), (n.theta, n.phi)))
            .collect();

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

                    for (id, sx, sy) in this.visualizer_hit_nodes.borrow().iter() {
                        let dx = *sx - click_x;
                        let dy = *sy - click_y;
                        let d_sq = dx * dx + dy * dy;
                        if d_sq < min_dist_sq {
                            min_dist_sq = d_sq;
                            clicked_quark = Some(id.clone());
                        }
                    }

                    if let Some(qid) = clicked_quark {
                        this.visualizer_selected_quark = Some(qid.clone());
                        if let Some(&(th, ph)) = node_angles_map.get(&qid) {
                            let (snap_yaw, snap_pitch) = focus_angles_for_spherical(th, ph);
                            this.visualizer_yaw = snap_yaw;
                            this.visualizer_pitch = snap_pitch;
                            this.visualizer_auto_spin = false;
                        }
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
            .children(activity_dock)
            .child(canvas_container)
            .children(selected_hud)
            .child(self.render_split_canvas_section(cx))
    }
}

/// Lavapipe-cached workspace crates & intra-crate modules topology graph.
#[derive(Debug, Clone, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct CachedCrateGraph {
    pub topology: hadron_forge::topology::WorkspaceTopologyGraph,
    pub crate_layers: BTreeMap<String, usize>,
    pub direct_modified_crates: HashSet<String>,
    pub downstream_impacted_crates: HashSet<String>,
    pub crate_positions: HashMap<String, (f32, f32)>,
}

pub const VISUALIZER_GRAPHS_CACHE_VERSION: u32 = 2;

const FNV_OFFSET_BASIS: u64 = 0xcbf29ce484222325;
const FNV_PRIME: u64 = 0x00000100000001b3;

fn fnv1a_update(mut hash: u64, bytes: &[u8]) -> u64 {
    for &b in bytes {
        hash ^= b as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

fn collect_rs_entries_meta(
    dir: &std::path::Path,
    rel_prefix: &str,
    out: &mut Vec<(String, u64, u128)>,
) {
    let Ok(read_dir) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in read_dir.flatten() {
        let file_name = entry.file_name();
        let Some(name_str) = file_name.to_str() else {
            continue;
        };
        if name_str.starts_with('.') {
            continue;
        }
        let Ok(file_type) = entry.file_type() else {
            continue;
        };
        let rel = format!("{}/{}", rel_prefix, name_str);
        if file_type.is_dir() {
            collect_rs_entries_meta(&entry.path(), &rel, out);
        } else if file_type.is_file() && name_str.ends_with(".rs") {
            if let Ok(meta) = entry.metadata() {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                out.push((rel, meta.len(), mtime));
            }
        }
    }
}

/// Computes a fast, deterministic 64-bit metadata fingerprint across `Cargo.toml`,
/// `crates/*/Cargo.toml`, and `crates/*/src/**/*.rs` without reading file contents.
pub fn compute_crate_graph_fingerprint(repo_root: &std::path::Path) -> u64 {
    let mut entries_meta: Vec<(String, u64, u128)> = Vec::new();

    let root_cargo = repo_root.join("Cargo.toml");
    if let Ok(meta) = std::fs::metadata(&root_cargo) {
        let mtime = meta
            .modified()
            .ok()
            .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
            .map(|d| d.as_nanos())
            .unwrap_or(0);
        entries_meta.push(("Cargo.toml".to_string(), meta.len(), mtime));
    }

    let crates_dir = repo_root.join("crates");
    if let Ok(read_dir) = std::fs::read_dir(&crates_dir) {
        for entry in read_dir.flatten() {
            let file_name = entry.file_name();
            let Some(dir_name) = file_name.to_str() else {
                continue;
            };
            if dir_name.is_empty() || dir_name.starts_with('.') || dir_name == "gpui-component" {
                continue;
            }
            let dir_path = entry.path();
            if !dir_path.is_dir() {
                continue;
            }
            let cargo_toml = dir_path.join("Cargo.toml");
            if let Ok(meta) = std::fs::metadata(&cargo_toml) {
                let mtime = meta
                    .modified()
                    .ok()
                    .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                    .map(|d| d.as_nanos())
                    .unwrap_or(0);
                entries_meta.push((format!("crates/{}/Cargo.toml", dir_name), meta.len(), mtime));
                let src_dir = dir_path.join("src");
                if src_dir.is_dir() {
                    collect_rs_entries_meta(
                        &src_dir,
                        &format!("crates/{}/src", dir_name),
                        &mut entries_meta,
                    );
                }
            }
        }
    }

    entries_meta.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hash = fnv1a_update(FNV_OFFSET_BASIS, &VISUALIZER_GRAPHS_CACHE_VERSION.to_le_bytes());
    for (rel, len, mtime) in entries_meta {
        hash = fnv1a_update(hash, rel.as_bytes());
        hash = fnv1a_update(hash, &[0xff]);
        hash = fnv1a_update(hash, &len.to_le_bytes());
        hash = fnv1a_update(hash, &mtime.to_le_bytes());
    }
    hash
}

/// Computes a fast, deterministic 64-bit metadata fingerprint across `.hadron/nucleus/index.md`,
/// `.hadron/nucleus/features.md`, `.hadron/nucleus/invariants/*.md`, and `.hadron/nucleus/notes/*.md`
/// using directory entry metadata.
pub fn compute_nucleus_graph_fingerprint(repo_root: &std::path::Path) -> u64 {
    let nucleus_dir = repo_root.join(".hadron").join("nucleus");
    let mut entries_meta: Vec<(String, u64, u128)> = Vec::new();

    for root_doc in ["index.md", "features.md", "invariants.md"] {
        let doc_path = nucleus_dir.join(root_doc);
        if let Ok(meta) = std::fs::metadata(&doc_path) {
            let mtime = meta
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_nanos())
                .unwrap_or(0);
            entries_meta.push((root_doc.to_string(), meta.len(), mtime));
        }
    }

    for subdir in ["notes", "invariants"] {
        let dir_path = nucleus_dir.join(subdir);
        if let Ok(read_dir) = std::fs::read_dir(&dir_path) {
            for entry in read_dir.flatten() {
                let file_name = entry.file_name();
                let Some(name_str) = file_name.to_str() else {
                    continue;
                };
                if !name_str.ends_with(".md") {
                    continue;
                };
                if let Ok(meta) = entry.metadata() {
                    let mtime = meta
                        .modified()
                        .ok()
                        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                        .map(|d| d.as_nanos())
                        .unwrap_or(0);
                    entries_meta.push((format!("{}/{}", subdir, name_str), meta.len(), mtime));
                }
            }
        }
    }

    entries_meta.sort_by(|a, b| a.0.cmp(&b.0));

    let mut hash = fnv1a_update(FNV_OFFSET_BASIS, &VISUALIZER_GRAPHS_CACHE_VERSION.to_le_bytes());
    for (rel, len, mtime) in entries_meta {
        hash = fnv1a_update(hash, rel.as_bytes());
        hash = fnv1a_update(hash, &[0xff]);
        hash = fnv1a_update(hash, &len.to_le_bytes());
        hash = fnv1a_update(hash, &mtime.to_le_bytes());
    }
    hash
}

/// On-disk cache for `Dependencies` (`CachedCrateGraph`) and `Nucleus Graph` (`CachedNucleusGraph`)
/// stored at `.hadron/cache/visualizer_graphs.json` for instant startup hydration while Hadron
/// checks for workspace changes in the background.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
pub struct VisualizerGraphsDiskCache {
    #[serde(default)]
    pub version: u32,
    #[serde(default)]
    pub crate_fingerprint: Option<u64>,
    #[serde(default)]
    pub crate_graph: Option<CachedCrateGraph>,
    #[serde(default)]
    pub nucleus_fingerprint: Option<u64>,
    #[serde(default)]
    pub nucleus_graph: Option<super::nucleus_graph::CachedNucleusGraph>,
}

impl VisualizerGraphsDiskCache {
    pub fn cache_path(repo_root: &std::path::Path) -> std::path::PathBuf {
        repo_root
            .join(".hadron")
            .join("cache")
            .join("visualizer_graphs.json")
    }

    pub fn load(repo_root: &std::path::Path) -> Self {
        let path = Self::cache_path(repo_root);
        let Ok(body) = std::fs::read_to_string(&path) else {
            return Self::default();
        };
        match serde_json::from_str::<Self>(&body) {
            Ok(cache) if cache.version == VISUALIZER_GRAPHS_CACHE_VERSION => cache,
            _ => Self::default(),
        }
    }

    pub fn save(&self, repo_root: &std::path::Path) -> std::io::Result<()> {
        let path = Self::cache_path(repo_root);
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut payload = self.clone();
        payload.version = VISUALIZER_GRAPHS_CACHE_VERSION;
        let tmp = path.with_extension(format!("json.{}.tmp", std::process::id()));
        let body = serde_json::to_string(&payload)
            .map_err(|e| std::io::Error::new(std::io::ErrorKind::InvalidData, e))?;
        std::fs::write(&tmp, body)?;
        std::fs::rename(&tmp, &path)
    }

    /// Compares workspace metadata fingerprints against `self`, rebuilds only missing or stale
    /// graphs, updates `self`, and persists `.hadron/cache/visualizer_graphs.json` atomically.
    pub fn revalidate(
        &mut self,
        repo_root: &std::path::Path,
        force_crate: bool,
        force_nucleus: bool,
    ) -> (
        Option<CachedCrateGraph>,
        Option<super::nucleus_graph::CachedNucleusGraph>,
    ) {
        let crate_fp = compute_crate_graph_fingerprint(repo_root);
        let nucleus_fp = compute_nucleus_graph_fingerprint(repo_root);

        let crate_stale =
            force_crate || self.crate_fingerprint != Some(crate_fp) || self.crate_graph.is_none();
        let nucleus_stale = force_nucleus
            || self.nucleus_fingerprint != Some(nucleus_fp)
            || self.nucleus_graph.is_none();

        let new_crate = if crate_stale {
            Some(CachedCrateGraph::build(repo_root))
        } else {
            None
        };
        let new_nucleus = if nucleus_stale {
            Some(super::nucleus_graph::CachedNucleusGraph::build(repo_root))
        } else {
            None
        };

        if new_crate.is_some() || new_nucleus.is_some() {
            if let Some(ref cg) = new_crate {
                self.crate_fingerprint = Some(crate_fp);
                self.crate_graph = Some(cg.clone());
            }
            if let Some(ref ng) = new_nucleus {
                self.nucleus_fingerprint = Some(nucleus_fp);
                self.nucleus_graph = Some(ng.clone());
            }
            if repo_root.join(".hadron").is_dir() {
                let _ = self.save(repo_root);
            }
        }

        (new_crate, new_nucleus)
    }
}

impl CachedCrateGraph {
    pub fn build(repo_root: &std::path::Path) -> Self {
        let topology = hadron_forge::topology::WorkspaceTopologyGraph::from_workspace(repo_root);
        let crate_layers = topology.compute_crate_layers();

        let direct_modified_crates = HashSet::new();
        let mut downstream_impacted_crates = HashSet::new();

        let crates_dir = repo_root.join("crates");
        if let Ok(entries) = std::fs::read_dir(&crates_dir) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.is_dir() {
                    if let Some(_crate_name) = p.file_name().and_then(|s| s.to_str()) {
                        let src_dir = p.join("src");
                        if src_dir.is_dir() {
                            // Crate directory exists
                        }
                    }
                }
            }
        }

        for edge in &topology.edges {
            if edge.relation == "depends_on" {
                if direct_modified_crates.contains(&edge.target_id) {
                    downstream_impacted_crates.insert(edge.source_id.clone());
                }
            }
        }

        let mut layer_groups: BTreeMap<usize, Vec<String>> = BTreeMap::new();
        for node in &topology.nodes {
            if let hadron_forge::topology::TopologyNode::Crate { name, .. } = node {
                let layer_ix = crate_layers.get(name).copied().unwrap_or(0);
                layer_groups.entry(layer_ix).or_default().push(name.clone());
            }
        }

        let mut crate_positions = HashMap::new();
        for (layer_ix, mut crates_in_layer) in layer_groups {
            crates_in_layer.sort();
            let y_pos = 100.0 + (layer_ix as f32 * 130.0);
            let count = crates_in_layer.len();
            let row_span = (count.saturating_sub(1) as f32) * 220.0;
            let start_x = 400.0 - row_span * 0.5;
            for (idx, crate_name) in crates_in_layer.into_iter().enumerate() {
                let x_pos = start_x + (idx as f32 * 220.0);
                crate_positions.insert(crate_name, (x_pos, y_pos));
            }
        }

        Self {
            topology,
            crate_layers,
            direct_modified_crates,
            downstream_impacted_crates,
            crate_positions,
        }
    }
}

/// Computes normalized viewport coordinates `(u, v)` in `[0.16, 0.84]` for each crate position
/// so that both the Bezier dependency canvas and the positioned crate node cards share the exact
/// same anchor points regardless of container resize.
pub(crate) fn compute_normalized_crate_coords(
    crate_positions: &HashMap<String, (f32, f32)>,
) -> HashMap<String, (f32, f32)> {
    if crate_positions.is_empty() {
        return HashMap::new();
    }

    let mut min_x = f32::MAX;
    let mut max_x = f32::MIN;
    let mut min_y = f32::MAX;
    let mut max_y = f32::MIN;

    for &(x, y) in crate_positions.values() {
        min_x = min_x.min(x);
        max_x = max_x.max(x);
        min_y = min_y.min(y);
        max_y = max_y.max(y);
    }

    let span_x = max_x - min_x;
    let span_y = max_y - min_y;

    let mut out = HashMap::with_capacity(crate_positions.len());
    for (name, &(x, y)) in crate_positions {
        let u = if span_x.abs() < 1e-3 {
            0.5
        } else {
            0.22 + ((x - min_x) / span_x) * 0.56
        };
        let v = if span_y.abs() < 1e-3 {
            0.5
        } else {
            0.16 + ((y - min_y) / span_y) * 0.68
        };
        out.insert(name.clone(), (u, v));
    }
    out
}

impl super::Chamber {
    pub(super) fn visualizer_mode_switcher_header(&self, cx: &mut Context<Self>) -> impl IntoElement {
        let current_mode = self.visualizer_mode;

        h_flex()
            .gap_1()
            .items_center()
            .p_1()
            .rounded_full()
            .bg(theme::tab_bar_bg())
            .border_1()
            .border_color(theme::glass_highlight())
            .children(VisualizerMode::ALL.map(|mode| {
                let is_active = current_mode == mode;
                let mode_label = mode.label();

                div()
                    .id(SharedString::from(format!("vis-mode-{}", mode.index())))
                    .px_3()
                    .py_1()
                    .rounded_full()
                    .cursor_pointer()
                    .bg(if is_active { theme::glass_highlight().into() } else { gpui::transparent_black() })
                    .text_xs()
                    .font_weight(if is_active { gpui::FontWeight::BOLD } else { gpui::FontWeight::MEDIUM })
                    .text_color(if is_active { theme::accent() } else { theme::text_muted() })
                    .hover(|s| if !is_active { s.text_color(theme::text()) } else { s })
                    .child(mode_label)
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.visualizer_mode = mode;
                        cx.notify();
                    }))
            }))
    }

    /// Hydrates `cached_crate_graph` and `cached_nucleus_graph` immediately from
    /// `.hadron/cache/visualizer_graphs.json` if not yet in memory, then verifies
    /// workspace metadata fingerprints in the background and rebuilds only stale graphs.
    pub(crate) fn warm_visualizer_graphs(&self, cx: &mut Context<Self>) {
        if self.cached_crate_graph_loading.get() || self.cached_nucleus_graph_loading.get() {
            return;
        }
        self.cached_crate_graph_loading.set(true);
        self.cached_nucleus_graph_loading.set(true);

        let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        let need_hydration = self.cached_crate_graph.borrow().is_none()
            || self.cached_nucleus_graph.borrow().is_none();

        cx.spawn(async move |this, cx| {
            // Phase 1: Fast disk cache load so both tabs render immediately at startup
            let repo_for_load = repo_root.clone();
            let mut disk_cache = cx
                .background_executor()
                .spawn(async move { VisualizerGraphsDiskCache::load(&repo_for_load) })
                .await;

            let mut had_crate_in_mem = !need_hydration;
            let mut had_nucleus_in_mem = !need_hydration;

            if need_hydration {
                let hydrated_crate = disk_cache.crate_graph.clone();
                let hydrated_nucleus = disk_cache.nucleus_graph.clone();
                let _ = this.update(cx, |chamber, cx| {
                    let mut hydrated = false;
                    if chamber.cached_crate_graph.borrow().is_none() {
                        if let Some(cg) = hydrated_crate {
                            *chamber.cached_crate_graph.borrow_mut() = Some(cg);
                            hydrated = true;
                        }
                    }
                    if chamber.cached_nucleus_graph.borrow().is_none() {
                        if let Some(ng) = hydrated_nucleus {
                            *chamber.cached_nucleus_graph.borrow_mut() = Some(ng);
                            hydrated = true;
                        }
                    }
                    had_crate_in_mem = chamber.cached_crate_graph.borrow().is_some();
                    had_nucleus_in_mem = chamber.cached_nucleus_graph.borrow().is_some();
                    if hydrated {
                        cx.notify();
                    }
                });
            }

            // Phase 2: Check metadata fingerprints in background and rebuild only stale graphs
            let repo_for_revalidate = repo_root.clone();
            let (rebuilt_crate, rebuilt_nucleus) = cx
                .background_executor()
                .spawn(async move {
                    disk_cache.revalidate(
                        &repo_for_revalidate,
                        !had_crate_in_mem,
                        !had_nucleus_in_mem,
                    )
                })
                .await;

            let _ = this.update(cx, |chamber, cx| {
                if let Some(cg) = rebuilt_crate {
                    *chamber.cached_crate_graph.borrow_mut() = Some(cg);
                }
                if let Some(ng) = rebuilt_nucleus {
                    *chamber.cached_nucleus_graph.borrow_mut() = Some(ng);
                }
                chamber.cached_nucleus_fingerprint = compute_nucleus_graph_fingerprint(&repo_root);
                chamber.cached_crate_graph_loading.set(false);
                chamber.cached_nucleus_graph_loading.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    /// Forces an asynchronous background rebuild of `CachedNucleusGraph` and updates
    /// `.hadron/cache/visualizer_graphs.json` whenever `.hadron/nucleus/` files change on disk
    /// or the user clicks `Refresh` in the `Nucleus Graph` header HUD.
    pub(crate) fn refresh_nucleus_graph(&self, cx: &mut Context<Self>) {
        self.cached_nucleus_graph_loading.set(true);
        let repo_root = crate::vcs::repo_root_of(&self.path).to_path_buf();
        cx.spawn(async move |this, cx| {
            let repo_for_build = repo_root.clone();
            let (rebuilt, fp) = cx
                .background_executor()
                .spawn(async move {
                    let built = super::nucleus_graph::CachedNucleusGraph::build_and_save(&repo_for_build);
                    let fp = compute_nucleus_graph_fingerprint(&repo_for_build);
                    (built, fp)
                })
                .await;

            let _ = this.update(cx, |chamber, cx| {
                *chamber.cached_nucleus_graph.borrow_mut() = Some(rebuilt);
                chamber.cached_nucleus_fingerprint = fp;
                chamber.cached_nucleus_graph_loading.set(false);
                cx.notify();
            });
        })
        .detach();
    }

    pub(super) fn crates_and_modules_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        if self.cached_crate_graph.borrow().is_none() {
            self.warm_visualizer_graphs(cx);
        }

        let cache_ref = self.cached_crate_graph.borrow();
        let mode_switcher = self.visualizer_mode_switcher_header(cx);

        if cache_ref.is_none() {
            return v_flex()
                .id("crates-modules-container")
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
                                .child("Building workspace dependency topology graph..."),
                        ),
                )
                .into_any_element();
        }

        let cached = cache_ref.as_ref().unwrap();
        let selected_crate = self.visualizer_selected_crate.clone();
        let is_revalidating = self.cached_crate_graph_loading.get();

        // Header Stats
        let hud_stats = h_flex()
            .gap_2()
            .items_center()
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
                    .child(format!("{} Packages", cached.crate_positions.len())),
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
                    .child(format!("{} Dependencies", cached.topology.edges.len())),
            )
            .when(!cached.direct_modified_crates.is_empty(), |this| {
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
                        .child(format!("{} Direct Modified", cached.direct_modified_crates.len())),
                )
            })
            .when(!cached.downstream_impacted_crates.is_empty(), |this| {
                this.child(
                    div()
                        .px_2p5()
                        .py_1()
                        .rounded_full()
                        .bg(theme::info().opacity(0.18))
                        .border_1()
                        .border_color(theme::info().opacity(0.4))
                        .text_xs()
                        .text_color(theme::info())
                        .child(format!("{} Downstream Impacted", cached.downstream_impacted_crates.len())),
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
            .child(hud_stats);

        // Canvas element with Bezier dependency curves and dynamically scaled layers
        let edges_list = cached.topology.edges.clone();
        let norm_coords = compute_normalized_crate_coords(&cached.crate_positions);
        let norm_coords_for_paint = norm_coords.clone();
        let sel_crate = selected_crate.clone();
        let direct_mod = cached.direct_modified_crates.clone();
        let downstream_imp = cached.downstream_impacted_crates.clone();

        let hit_bounds_cell = std::rc::Rc::new(std::cell::RefCell::new(Vec::<(String, Bounds<Pixels>)>::new()));
        let hit_bounds_for_paint = hit_bounds_cell.clone();

        let canvas_elem = canvas(
            move |bounds, _, _| bounds,
            move |bounds, _, window, _cx| {
                let w = f32::from(bounds.size.width);
                let h = f32::from(bounds.size.height);
                let origin_x = f32::from(bounds.origin.x);
                let origin_y = f32::from(bounds.origin.y);

                let project_uv = |u: f32, v: f32| -> (f32, f32) {
                    (origin_x + w * u, origin_y + h * v)
                };

                let mut painted_hits = Vec::new();

                // Draw Bezier edges
                for edge in &edges_list {
                    if edge.relation == "depends_on" {
                        if let (Some(&(u1, v1)), Some(&(u2, v2))) = (
                            norm_coords_for_paint.get(&edge.source_id),
                            norm_coords_for_paint.get(&edge.target_id),
                        ) {
                            let (sx1, sy1) = project_uv(u1, v1);
                            let (sx2, sy2) = project_uv(u2, v2);

                            let mut builder = PathBuilder::stroke(px(1.75));
                            builder.move_to(point(px(sx1), px(sy1)));
                            let mid_y = (sy1 + sy2) * 0.5;
                            builder.cubic_bezier_to(
                                point(px(sx2), px(sy2)),
                                point(px(sx1), px(mid_y)),
                                point(px(sx2), px(mid_y)),
                            );

                            if let Ok(path) = builder.build() {
                                let is_sel_edge = sel_crate.as_deref() == Some(edge.source_id.as_str())
                                    || sel_crate.as_deref() == Some(edge.target_id.as_str());
                                let edge_color = if is_sel_edge {
                                    theme::accent().opacity(0.90)
                                } else if direct_mod.contains(&edge.target_id) {
                                    gpui::rgb(0xe5c07b).opacity(0.8)
                                } else if downstream_imp.contains(&edge.source_id) {
                                    gpui::rgb(0x61afef).opacity(0.7)
                                } else {
                                    gpui::rgb(0xa87ffb).opacity(0.40)
                                };
                                window.paint_path(path, edge_color);
                            }
                        }
                    }
                }

                // Record hit bounds for each crate node card
                for (crate_name, &(u, v)) in &norm_coords_for_paint {
                    let (sx, sy) = project_uv(u, v);
                    let card_w = px(168.0);
                    let card_h = px(52.0);
                    let node_bounds = Bounds {
                        origin: point(px(sx) - card_w / 2.0, px(sy) - card_h / 2.0),
                        size: size(card_w, card_h),
                    };
                    painted_hits.push((crate_name.clone(), node_bounds));
                }

                if let Ok(mut cell) = hit_bounds_for_paint.try_borrow_mut() {
                    *cell = painted_hits;
                }
            },
        )
        .size_full();

        let hit_bounds_for_click = hit_bounds_cell.clone();

        // Positioned interactive crate node cards with package name, layer badge, and dependency count
        let mut sorted_crates: Vec<String> = norm_coords.keys().cloned().collect();
        sorted_crates.sort();
        let node_cards = sorted_crates.into_iter().filter_map(|c_name| {
            let (u, v) = norm_coords.get(&c_name).copied()?;
            let layer_ix = cached.crate_layers.get(&c_name).copied().unwrap_or(0);
            let dep_count = cached
                .topology
                .edges
                .iter()
                .filter(|e| e.relation == "depends_on" && e.source_id == c_name)
                .count();
            let rdep_count = cached
                .topology
                .edges
                .iter()
                .filter(|e| e.relation == "depends_on" && e.target_id == c_name)
                .count();
            let is_sel = selected_crate.as_deref() == Some(c_name.as_str());
            let is_direct = cached.direct_modified_crates.contains(&c_name);
            let is_impacted = cached.downstream_impacted_crates.contains(&c_name);

            let card_bg = if is_sel {
                theme::tab_bar_bg()
            } else if is_direct {
                theme::warning().opacity(0.18)
            } else if is_impacted {
                theme::info().opacity(0.18)
            } else {
                theme::bg_surface()
            };

            let border_col: Hsla = if is_sel {
                theme::accent().into()
            } else if is_direct {
                theme::warning().opacity(0.55).into()
            } else if is_impacted {
                theme::info().opacity(0.55).into()
            } else {
                theme::glass_highlight().into()
            };

            let click_name = c_name.clone();
            Some(
                v_flex()
                    .id(SharedString::from(format!("crate-node-card-{}", c_name)))
                    .absolute()
                    .left(gpui::relative(u))
                    .top(gpui::relative(v))
                    .ml(px(-84.0))
                    .mt(px(-26.0))
                    .w(px(168.0))
                    .h(px(52.0))
                    .px_2p5()
                    .py_1p5()
                    .justify_between()
                    .rounded_lg()
                    .cursor_pointer()
                    .bg(card_bg)
                    .border_1()
                    .border_color(border_col)
                    .hover(|s| s.border_color(theme::accent()))
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(if is_sel { theme::accent() } else { theme::text() })
                                    .truncate()
                                    .child(c_name.clone()),
                            )
                            .child(
                                div()
                                    .px_1p5()
                                    .py_0p5()
                                    .rounded_sm()
                                    .bg(theme::tab_bar_bg())
                                    .text_xs()
                                    .text_color(theme::text_muted())
                                    .child(format!("L{}", layer_ix)),
                            ),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .justify_between()
                            .items_center()
                            .text_xs()
                            .text_color(theme::text_muted())
                            .child(format!("{} deps · {} rdeps", dep_count, rdep_count))
                            .when(is_direct, |row| {
                                row.child(div().text_color(theme::warning()).child("● mod"))
                            })
                            .when(!is_direct && is_impacted, |row| {
                                row.child(div().text_color(theme::info()).child("● imp"))
                            }),
                    )
                    .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.visualizer_selected_crate = Some(click_name.clone());
                        cx.notify();
                    })),
            )
        });

        let canvas_container = div()
            .relative()
            .flex_1()
            .min_h_0()
            .cursor_pointer()
            .on_mouse_down(
                MouseButton::Left,
                cx.listener(move |this, ev: &gpui::MouseDownEvent, _window, cx| {
                    let click_pos = ev.position;
                    let mut clicked: Option<String> = None;

                    for (c_name, node_bounds) in hit_bounds_for_click.borrow().iter() {
                        if node_bounds.contains(&click_pos) {
                            clicked = Some(c_name.clone());
                            break;
                        }
                    }

                    if let Some(c_name) = clicked {
                        this.visualizer_selected_crate = Some(c_name);
                        cx.notify();
                    }
                }),
            )
            .child(canvas_elem)
            .children(node_cards);

        // Crate Chips Dock (top-anchored below header so selecting crates never shifts bottom layout)
        let crate_chips_dock = h_flex()
            .id("crates-dock")
            .w_full()
            .min_w_0()
            .overflow_hidden()
            .flex_wrap()
            .px_3()
            .py_2()
            .gap_1p5()
            .border_b_1()
            .border_color(theme::glass_highlight())
            .bg(theme::bg_surface().opacity(0.40))
            .children(cached.crate_positions.keys().map(|c_name| {
                let name = c_name.clone();
                let is_sel = selected_crate.as_deref() == Some(&name);
                let is_direct = cached.direct_modified_crates.contains(&name);
                let is_impacted = cached.downstream_impacted_crates.contains(&name);

                let chip_bg = if is_sel {
                    theme::tab_bar_bg()
                } else if is_direct {
                    theme::warning().opacity(0.15)
                } else if is_impacted {
                    theme::info().opacity(0.15)
                } else {
                    theme::tab_bar_bg().opacity(0.60)
                };

                h_flex()
                    .id(SharedString::from(format!("crate-chip-{}", name)))
                    .items_center()
                    .gap_1p5()
                    .px_2p5()
                    .py_1()
                    .rounded_md()
                    .cursor_pointer()
                    .bg(chip_bg)
                    .border_1()
                    .border_color(if is_sel { theme::accent() } else { theme::glass_highlight().into() })
                    .child(
                        div()
                            .text_xs()
                            .font_weight(gpui::FontWeight::MEDIUM)
                            .text_color(if is_sel { theme::text() } else { theme::text_secondary() })
                            .child(name.clone()),
                    )
                    .when(is_direct, |chip| {
                        chip.child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(theme::warning().opacity(0.2))
                                .text_xs()
                                .text_color(theme::warning())
                                .child("modified"),
                        )
                    })
                    .when(is_impacted, |chip| {
                        chip.child(
                            div()
                                .px_1p5()
                                .py_0p5()
                                .rounded_sm()
                                .bg(theme::info().opacity(0.2))
                                .text_xs()
                                .text_color(theme::info())
                                .child("impacted"),
                        )
                    })
                    .on_click(cx.listener(move |this, _, _, cx| {
                        this.visualizer_selected_crate = Some(name.clone());
                        cx.notify();
                    }))
            }));

        // Selected Crate & Intra-Crate Module Drill-Down Inspector Dock (full-width bottom dock)
        let crate_inspector = selected_crate.as_ref().map(|c_name| {
            let layer_ix = cached.crate_layers.get(c_name).copied().unwrap_or(0);
            let deps: Vec<String> = cached.topology.edges.iter()
                .filter(|e| e.relation == "depends_on" && e.source_id == *c_name)
                .map(|e| e.target_id.clone())
                .collect();
            let dependents: Vec<String> = cached.topology.edges.iter()
                .filter(|e| e.relation == "depends_on" && e.target_id == *c_name)
                .map(|e| e.source_id.clone())
                .collect();

            let crate_rel_dir = cached
                .topology
                .nodes
                .iter()
                .find_map(|n| match n {
                    hadron_forge::topology::TopologyNode::Crate { name, path } if name == c_name => {
                        Some(path.clone())
                    }
                    _ => None,
                })
                .unwrap_or_else(|| format!("crates/{}", c_name));

            let module_prefix = format!("{}/", crate_rel_dir);
            let modules: Vec<&hadron_forge::topology::CrateModuleNode> = cached.topology.crate_modules.iter()
                .filter(|m| m.rel_path.starts_with(&module_prefix))
                .collect();

            v_flex()
                .id("crate-inspector-drawer")
                .w_full()
                .max_h(px(164.0))
                .overflow_y_scroll()
                .p_3()
                .gap_2()
                .bg(theme::bg_surface().opacity(0.65))
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
                                        .child(c_name.clone()),
                                )
                                .child(
                                    div()
                                        .px_2()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(theme::tab_bar_bg())
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(format!("Layer L{}", layer_ix)),
                                ),
                        )
                        .child(
                            h_flex()
                                .gap_3()
                                .items_center()
                                .child(
                                    div()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .child(crate_rel_dir),
                                )
                                .child(
                                    div()
                                        .id("close-crate-inspector")
                                        .px_2()
                                        .py_0p5()
                                        .rounded_md()
                                        .cursor_pointer()
                                        .text_xs()
                                        .text_color(theme::text_muted())
                                        .hover(|s| s.text_color(theme::text()))
                                        .child("✕")
                                        .on_click(cx.listener(|this, _, _, cx| {
                                            this.visualizer_selected_crate = None;
                                            cx.notify();
                                        })),
                                ),
                        ),
                )
                .child(
                    h_flex()
                        .gap_4()
                        .items_center()
                        .text_xs()
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(div().text_color(theme::text_muted()).child("Dependencies:"))
                                 .children(deps.iter().map(|d| {
                                    let dep_name = d.clone();
                                    div()
                                        .id(SharedString::from(format!("dep-{}", dep_name)))
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(theme::bg_surface())
                                        .text_color(theme::text())
                                        .cursor_pointer()
                                        .child(dep_name.clone())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.visualizer_selected_crate = Some(dep_name.clone());
                                            cx.notify();
                                        }))
                                })),
                        )
                        .child(
                            h_flex()
                                .gap_1p5()
                                .items_center()
                                .child(div().text_color(theme::text_muted()).child("Dependents:"))
                                .children(dependents.iter().map(|d| {
                                    let dep_name = d.clone();
                                    div()
                                        .id(SharedString::from(format!("dependent-{}", dep_name)))
                                        .px_1p5()
                                        .py_0p5()
                                        .rounded_sm()
                                        .bg(theme::bg_surface())
                                        .text_color(theme::text())
                                        .cursor_pointer()
                                        .child(dep_name.clone())
                                        .on_click(cx.listener(move |this, _, _, cx| {
                                            this.visualizer_selected_crate = Some(dep_name.clone());
                                            cx.notify();
                                        }))
                                })),
                        ),
                )
                .when(!modules.is_empty(), |this| {
                    this.child(
                        v_flex()
                            .gap_1()
                            .child(
                                div()
                                    .text_xs()
                                    .font_weight(gpui::FontWeight::BOLD)
                                    .text_color(theme::text_secondary())
                                    .child("Intra-Crate Module Structure:"),
                            )
                            .child(
                                h_flex()
                                    .flex_wrap()
                                    .gap_1p5()
                                    .children(modules.iter().map(|m| {
                                        div()
                                            .px_2()
                                            .py_1()
                                            .rounded_md()
                                            .bg(theme::bg_surface())
                                            .border_1()
                                            .border_color(theme::glass_highlight())
                                            .text_xs()
                                            .text_color(theme::text())
                                            .child(format!("{} ({} imports)", m.name, m.imports.len()))
                                    })),
                            ),
                    )
                })
        });

        v_flex()
            .id("crates-modules-container")
            .size_full()
            .bg(theme::canvas_base())
            .child(header)
            .child(crate_chips_dock)
            .child(canvas_container)
            .children(crate_inspector)
            .into_any_element()
    }

    /// Primary visualizer entrypoint dispatching across the 3 visualizer modes.
    pub(super) fn visualizer_view(&self, cx: &mut Context<Self>) -> impl IntoElement {
        match self.visualizer_mode {
            VisualizerMode::Swarm3D => self.swarm_3d_view(cx).into_any_element(),
            VisualizerMode::CratesAndModules => self.crates_and_modules_view(cx).into_any_element(),
            VisualizerMode::NucleusGraph => self.nucleus_graph_view(cx).into_any_element(),
        }
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

    #[test]
    fn focus_angles_rotates_to_front_hemisphere() {
        let test_cases = [
            (0.0f32, 0.0f32),
            (0.5, 1.2),
            (-0.4, 2.8),
            (1.12, 0.0), // Orchestrator apex
            (-0.75, 4.5),
            (0.3, 5.8),
            (-1.2, 3.14),
        ];

        for (theta, phi) in test_cases {
            let (yaw, pitch) = focus_angles_for_spherical(theta, phi);
            let p = Point3D::from_spherical(100.0, theta, phi);
            let rot_p = Rotation3D::new(yaw, pitch).transform(p);

            assert!(
                rot_p.z > 0.0,
                "Point rotated by focus_angles_for_spherical must be in front hemisphere (z > 0), got z={}",
                rot_p.z
            );
            assert!(
                rot_p.x.abs() < 1e-2,
                "Point rotated by focus_angles_for_spherical must be centered on X, got x={}",
                rot_p.x
            );
            assert!(
                rot_p.y.abs() < 1e-2,
                "Point rotated by focus_angles_for_spherical must be centered on Y, got y={}",
                rot_p.y
            );
        }
    }

    #[test]
    fn phase_halo_color_selection() {
        assert_eq!(
            phase_halo_color(Some(hadron_lattice::live::Doing::Thinking)),
            theme::halo_reasoning()
        );
        assert_eq!(
            phase_halo_color(Some(hadron_lattice::live::Doing::Working)),
            theme::success().into()
        );
        assert_eq!(
            phase_halo_color(Some(hadron_lattice::live::Doing::Planning)),
            theme::halo_reasoning()
        );
        assert_eq!(
            phase_halo_color(Some(hadron_lattice::live::Doing::Speaking)),
            theme::warning().into()
        );
        assert_eq!(
            phase_halo_color(Some(hadron_lattice::live::Doing::Gating)),
            theme::info().into()
        );
        assert_eq!(phase_halo_color(None), theme::success().into());
    }

    #[test]
    fn dual_shell_topology_radius_scaling() {
        let base_radius = 160.0;
        let inner_r = base_radius * 1.0;
        let outer_r = base_radius * 1.28;

        let p_inner = Point3D::from_spherical(inner_r, 0.5, 1.0);
        let p_outer = Point3D::from_spherical(outer_r, 0.5, 1.0);

        let dist_inner = (p_inner.x * p_inner.x + p_inner.y * p_inner.y + p_inner.z * p_inner.z).sqrt();
        let dist_outer = (p_outer.x * p_outer.x + p_outer.y * p_outer.y + p_outer.z * p_outer.z).sqrt();

        assert!((dist_inner - 160.0).abs() < 1e-3);
        assert!((dist_outer - 204.8).abs() < 1e-3);
        assert!(dist_outer > dist_inner);
    }

    #[test]
    fn normalized_crate_coords_spreads_layers_cleanly() {
        let mut raw = HashMap::new();
        raw.insert("hadron-lattice".to_string(), (400.0, 100.0));
        raw.insert("hadron-forge".to_string(), (290.0, 230.0));
        raw.insert("hadron-gatekeeper".to_string(), (510.0, 230.0));
        raw.insert("hadron".to_string(), (400.0, 490.0));

        let coords = compute_normalized_crate_coords(&raw);
        assert_eq!(coords.len(), 4);

        let (u_lattice, v_lattice) = coords["hadron-lattice"];
        let (u_forge, v_forge) = coords["hadron-forge"];
        let (u_gatekeeper, v_gatekeeper) = coords["hadron-gatekeeper"];
        let (u_hadron, v_hadron) = coords["hadron"];

        assert!((u_lattice - 0.5).abs() < 1e-3);
        assert!((u_hadron - 0.5).abs() < 1e-3);
        assert!(u_forge < u_gatekeeper);
        assert!((v_forge - v_gatekeeper).abs() < 1e-3);
        assert!(v_lattice < v_forge && v_forge < v_hadron);
    }

    #[test]
    fn visualizer_graphs_disk_cache_roundtrip_and_fingerprint_invalidation() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path();

        // Setup minimal workspace + .hadron/nucleus
        let crate_src = root.join("crates/alpha/src");
        let notes_dir = root.join(".hadron/nucleus/notes");
        std::fs::create_dir_all(&crate_src).unwrap();
        std::fs::create_dir_all(&notes_dir).unwrap();

        std::fs::write(
            root.join("Cargo.toml"),
            "[workspace]\nmembers = [\"crates/alpha\"]\n",
        )
        .unwrap();
        std::fs::write(
            root.join("crates/alpha/Cargo.toml"),
            "[package]\nname = \"alpha\"\nversion = \"0.1.0\"\n",
        )
        .unwrap();
        std::fs::write(crate_src.join("lib.rs"), "pub mod sub;\n").unwrap();

        std::fs::write(
            root.join(".hadron/nucleus/index.md"),
            "- [note-a](notes/note-a.md) — First note\n",
        )
        .unwrap();
        std::fs::write(
            notes_dir.join("note-a.md"),
            "---\nname: note-a\ndescription: First note\n---\nHello world.\n",
        )
        .unwrap();

        let crate_fp_1 = compute_crate_graph_fingerprint(root);
        let nucleus_fp_1 = compute_nucleus_graph_fingerprint(root);
        assert_eq!(crate_fp_1, compute_crate_graph_fingerprint(root));
        assert_eq!(nucleus_fp_1, compute_nucleus_graph_fingerprint(root));

        // Initial revalidate populates .hadron/cache/visualizer_graphs.json
        let mut cache = VisualizerGraphsDiskCache::load(root);
        let (cg_1_opt, ng_1_opt) = cache.revalidate(root, false, false);
        let cg_1 = cg_1_opt.expect("cold cache must build crate graph");
        let ng_1 = ng_1_opt.expect("cold cache must build nucleus graph");
        assert!(VisualizerGraphsDiskCache::cache_path(root).is_file());

        // Second revalidate with unchanged files is a pure cache hit (returns None, None)
        let mut disk = VisualizerGraphsDiskCache::load(root);
        assert_eq!(disk.version, VISUALIZER_GRAPHS_CACHE_VERSION);
        assert_eq!(disk.crate_fingerprint, Some(crate_fp_1));
        assert_eq!(disk.nucleus_fingerprint, Some(nucleus_fp_1));
        assert_eq!(disk.crate_graph.as_ref(), Some(&cg_1));
        assert_eq!(disk.nucleus_graph.as_ref(), Some(&ng_1));
        let (hit_cg, hit_ng) = disk.revalidate(root, false, false);
        assert!(hit_cg.is_none(), "unchanged crate graph should not rebuild");
        assert!(hit_ng.is_none(), "unchanged nucleus graph should not rebuild");

        // Mutating a note invalidates only the nucleus fingerprint and rebuilds only Nucleus Graph
        std::fs::write(
            notes_dir.join("note-b.md"),
            "---\nname: note-b\ndescription: Second note\n---\nLinks to [[note-a]].\n",
        )
        .unwrap();
        let nucleus_fp_2 = compute_nucleus_graph_fingerprint(root);
        assert_ne!(nucleus_fp_1, nucleus_fp_2);
        assert_eq!(crate_fp_1, compute_crate_graph_fingerprint(root));

        let (rebuilt_cg, rebuilt_ng) = disk.revalidate(root, false, false);
        assert!(rebuilt_cg.is_none(), "crate graph should stay cached when only notes change");
        let ng_2 = rebuilt_ng.expect("nucleus graph must rebuild when a note is added");
        assert_eq!(ng_2.nodes.len(), 2);
        let disk_after_note = VisualizerGraphsDiskCache::load(root);
        assert_eq!(disk_after_note.nucleus_fingerprint, Some(nucleus_fp_2));
        assert_eq!(disk_after_note.crate_fingerprint, Some(crate_fp_1));

        // Mutating invariants/always.md also invalidates the nucleus fingerprint
        let inv_dir = root.join(".hadron/nucleus/invariants");
        std::fs::create_dir_all(&inv_dir).unwrap();
        std::fs::write(
            inv_dir.join("always.md"),
            "- Constraint → `notes/note-b.md`\n",
        )
        .unwrap();
        let nucleus_fp_3 = compute_nucleus_graph_fingerprint(root);
        assert_ne!(nucleus_fp_2, nucleus_fp_3);

        // Mutating a crate source file invalidates only the crate fingerprint and rebuilds only Crate Graph
        let mut disk_inv = VisualizerGraphsDiskCache::load(root);
        let (_, rebuilt_ng_inv) = disk_inv.revalidate(root, false, false);
        assert!(rebuilt_ng_inv.is_some(), "nucleus graph must rebuild when invariants/always.md changes");
        std::fs::write(crate_src.join("extra.rs"), "use crate::sub;\n").unwrap();
        let crate_fp_2 = compute_crate_graph_fingerprint(root);
        assert_ne!(crate_fp_1, crate_fp_2);

        let mut disk_2 = VisualizerGraphsDiskCache::load(root);
        let (rebuilt_cg_2, rebuilt_ng_2) = disk_2.revalidate(root, false, false);
        assert!(rebuilt_ng_2.is_none(), "nucleus graph should stay cached when only crate src changes");
        let cg_2 = rebuilt_cg_2.expect("crate graph must rebuild when src changes");
        assert_eq!(cg_2.topology.crate_modules.len(), 2);
        let disk_after_crate = VisualizerGraphsDiskCache::load(root);
        assert_eq!(disk_after_crate.crate_fingerprint, Some(crate_fp_2));
    }
}
