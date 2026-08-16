//! 2D layout calculation and geometry engine for Mermaid diagrams.

use super::ast::*;
use std::collections::{HashMap, HashSet, VecDeque};

/// 2D Point
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// 2D Rectangle Bounds
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    pub fn right(&self) -> f32 {
        self.x + self.width
    }
    pub fn bottom(&self) -> f32 {
        self.y + self.height
    }
    pub fn center_x(&self) -> f32 {
        self.x + self.width / 2.0
    }
    pub fn center_y(&self) -> f32 {
        self.y + self.height / 2.0
    }
}

/// Computed visual layout of a Flowchart diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowchartLayout {
    pub canvas_width: f32,
    pub canvas_height: f32,
    pub nodes: Vec<PositionedNode>,
    pub edges: Vec<PositionedEdge>,
    pub subgraphs: Vec<PositionedSubgraph>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedNode {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
    pub bounds: Rect,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedEdge {
    pub from_id: String,
    pub to_id: String,
    pub start: Point,
    pub end: Point,
    pub control1: Point,
    pub control2: Point,
    pub label: Option<String>,
    pub label_pos: Point,
    pub style: EdgeStyle,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedSubgraph {
    pub id: String,
    pub title: String,
    pub bounds: Rect,
}

/// Computed visual layout of a Sequence diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SequenceLayout {
    pub canvas_width: f32,
    pub canvas_height: f32,
    pub participants: Vec<PositionedParticipant>,
    pub messages: Vec<PositionedMessage>,
    pub notes: Vec<PositionedNote>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedParticipant {
    pub id: String,
    pub label: String,
    pub header_bounds: Rect,
    pub lifeline_x: f32,
    pub is_actor: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedMessage {
    pub start: Point,
    pub end: Point,
    pub text: String,
    pub is_dotted: bool,
    pub is_arrow: bool,
    pub is_cross: bool,
    pub number: Option<usize>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedNote {
    pub bounds: Rect,
    pub text: String,
}

/// Computed visual layout of a Pie chart.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PieLayout {
    pub title: Option<String>,
    pub total: f64,
    pub slices: Vec<PositionedPieSlice>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PositionedPieSlice {
    pub label: String,
    pub value: f64,
    pub percentage: f64,
    pub color_index: usize,
}

/// Layout computation entry point.
pub fn compute_layout(diagram: &MermaidDiagram) -> LayoutResult {
    match diagram {
        MermaidDiagram::Flowchart(f) => LayoutResult::Flowchart(layout_flowchart(f)),
        MermaidDiagram::Sequence(s) => LayoutResult::Sequence(layout_sequence(s)),
        MermaidDiagram::Pie(p) => LayoutResult::Pie(layout_pie(p)),
        MermaidDiagram::State(s) => LayoutResult::Flowchart(layout_state_as_flowchart(s)),
        MermaidDiagram::Class(c) => LayoutResult::Flowchart(layout_class_as_flowchart(c)),
        MermaidDiagram::Raw { diagram_type, source } => LayoutResult::Raw {
            diagram_type: diagram_type.clone(),
            source: source.clone(),
        },
    }
}

pub enum LayoutResult {
    Flowchart(FlowchartLayout),
    Sequence(SequenceLayout),
    Pie(PieLayout),
    Raw { diagram_type: String, source: String },
}

const PAD: f32 = 24.0;
const NODE_H: f32 = 38.0;
const LAYER_GAP: f32 = 64.0;
const SIBLING_GAP: f32 = 24.0;

fn estimate_node_width(label: &str) -> f32 {
    let char_len = label.chars().count();
    (char_len as f32 * 8.5 + 32.0).max(96.0).min(320.0)
}

fn layout_flowchart(diagram: &FlowchartDiagram) -> FlowchartLayout {
    if diagram.nodes.is_empty() {
        return FlowchartLayout::default();
    }

    // Step 1: Layer Assignment (Topological Ranking)
    let mut in_degrees: HashMap<String, usize> = HashMap::new();
    let mut adj: HashMap<String, Vec<String>> = HashMap::new();

    for id in diagram.nodes.keys() {
        in_degrees.insert(id.clone(), 0);
        adj.insert(id.clone(), Vec::new());
    }

    for edge in &diagram.edges {
        if diagram.nodes.contains_key(&edge.from) && diagram.nodes.contains_key(&edge.to) {
            *in_degrees.entry(edge.to.clone()).or_insert(0) += 1;
            adj.entry(edge.from.clone()).or_default().push(edge.to.clone());
        }
    }

    // Assign layers via BFS
    let mut layers: HashMap<String, usize> = HashMap::new();
    let mut queue: VecDeque<(String, usize)> = VecDeque::new();
    let mut visited: HashSet<String> = HashSet::new();

    // Start with root nodes
    for (id, &deg) in &in_degrees {
        if deg == 0 {
            queue.push_back((id.clone(), 0));
            visited.insert(id.clone());
            layers.insert(id.clone(), 0);
        }
    }

    // If cycle or no root nodes, enqueue first unvisited node
    if queue.is_empty() {
        if let Some(first) = diagram.node_order.first().or_else(|| diagram.nodes.keys().next()) {
            queue.push_back((first.clone(), 0));
            visited.insert(first.clone());
            layers.insert(first.clone(), 0);
        }
    }

    while let Some((node_id, layer)) = queue.pop_front() {
        if let Some(neighbors) = adj.get(&node_id) {
            for neighbor in neighbors {
                let next_layer = layer + 1;
                let entry = layers.entry(neighbor.clone()).or_insert(0);
                if next_layer > *entry {
                    *entry = next_layer;
                }
                if !visited.contains(neighbor) {
                    visited.insert(neighbor.clone());
                    queue.push_back((neighbor.clone(), next_layer));
                }
            }
        }
    }

    // Assign any disconnected nodes to layer 0
    for id in diagram.nodes.keys() {
        if !layers.contains_key(id) {
            layers.insert(id.clone(), 0);
        }
    }

    // Group nodes by layer
    let max_layer = layers.values().copied().max().unwrap_or(0);
    let mut layer_groups: Vec<Vec<String>> = vec![Vec::new(); max_layer + 1];

    for id in &diagram.node_order {
        if let Some(&layer) = layers.get(id) {
            if !layer_groups[layer].contains(id) {
                layer_groups[layer].push(id.clone());
            }
        }
    }
    for id in diagram.nodes.keys() {
        if let Some(&layer) = layers.get(id) {
            if !layer_groups[layer].contains(id) {
                layer_groups[layer].push(id.clone());
            }
        }
    }

    // Compute max width or height for grid balance
    let is_horizontal = matches!(
        diagram.direction,
        Direction::LeftRight | Direction::RightLeft
    );

    let mut positioned_nodes_map: HashMap<String, Rect> = HashMap::new();
    let mut max_x = 0.0f32;
    let mut max_y = 0.0f32;

    if is_horizontal {
        // Horizontal Layout: Layers are Columns along X, Nodes in layer along Y
        let col_widths: Vec<f32> = layer_groups
            .iter()
            .map(|group| {
                group
                    .iter()
                    .map(|id| {
                        let label = diagram.nodes.get(id).map_or("", |n| &n.label);
                        estimate_node_width(label)
                    })
                    .max_by(|a, b| a.partial_cmp(b).unwrap())
                    .unwrap_or(120.0)
            })
            .collect();

        // Calculate max column height to center shorter columns
        let col_heights: Vec<f32> = layer_groups
            .iter()
            .map(|group| {
                let n = group.len();
                if n == 0 {
                    0.0
                } else {
                    (n as f32) * NODE_H + ((n - 1) as f32) * SIBLING_GAP
                }
            })
            .collect();
        let max_col_h = col_heights.iter().copied().fold(0.0f32, f32::max);

        let mut cur_x = PAD;
        for (layer_ix, group) in layer_groups.iter().enumerate() {
            let col_w = col_widths[layer_ix];
            let col_h = col_heights[layer_ix];
            let start_y = PAD + (max_col_h - col_h) / 2.0;

            let mut cur_y = start_y;
            for id in group {
                let label = diagram.nodes.get(id).map_or("", |n| &n.label);
                let w = estimate_node_width(label);
                let x = cur_x + (col_w - w) / 2.0;
                let rect = Rect {
                    x,
                    y: cur_y,
                    width: w,
                    height: NODE_H,
                };
                positioned_nodes_map.insert(id.clone(), rect);
                max_x = max_x.max(rect.right());
                max_y = max_y.max(rect.bottom());
                cur_y += NODE_H + SIBLING_GAP;
            }
            cur_x += col_w + LAYER_GAP;
        }
    } else {
        // Vertical Layout: Layers are Rows along Y, Nodes in layer along X
        let row_widths: Vec<f32> = layer_groups
            .iter()
            .map(|group| {
                let total_w: f32 = group
                    .iter()
                    .map(|id| {
                        let label = diagram.nodes.get(id).map_or("", |n| &n.label);
                        estimate_node_width(label)
                    })
                    .sum();
                let gaps = if group.len() > 1 {
                    (group.len() - 1) as f32 * SIBLING_GAP
                } else {
                    0.0
                };
                total_w + gaps
            })
            .collect();
        let max_row_w = row_widths.iter().copied().fold(0.0f32, f32::max);

        let mut cur_y = PAD;
        for (layer_ix, group) in layer_groups.iter().enumerate() {
            let row_w = row_widths[layer_ix];
            let start_x = PAD + (max_row_w - row_w) / 2.0;

            let mut cur_x = start_x;
            for id in group {
                let label = diagram.nodes.get(id).map_or("", |n| &n.label);
                let w = estimate_node_width(label);
                let rect = Rect {
                    x: cur_x,
                    y: cur_y,
                    width: w,
                    height: NODE_H,
                };
                positioned_nodes_map.insert(id.clone(), rect);
                max_x = max_x.max(rect.right());
                max_y = max_y.max(rect.bottom());
                cur_x += w + SIBLING_GAP;
            }
            cur_y += NODE_H + LAYER_GAP;
        }
    }

    // Step 2: Build Positioned Nodes list
    let mut positioned_nodes = Vec::new();
    for id in &diagram.node_order {
        if let (Some(node), Some(&bounds)) = (diagram.nodes.get(id), positioned_nodes_map.get(id)) {
            positioned_nodes.push(PositionedNode {
                id: id.clone(),
                label: node.label.clone(),
                shape: node.shape,
                bounds,
            });
        }
    }
    for (id, node) in &diagram.nodes {
        if !positioned_nodes.iter().any(|n| &n.id == id) {
            if let Some(&bounds) = positioned_nodes_map.get(id) {
                positioned_nodes.push(PositionedNode {
                    id: id.clone(),
                    label: node.label.clone(),
                    shape: node.shape,
                    bounds,
                });
            }
        }
    }

    // Step 3: Route Edges & Connectors
    let mut positioned_edges = Vec::new();
    for edge in &diagram.edges {
        let (Some(&from_bounds), Some(&to_bounds)) = (
            positioned_nodes_map.get(&edge.from),
            positioned_nodes_map.get(&edge.to),
        ) else {
            continue;
        };

        let (start, end) = if is_horizontal {
            if from_bounds.center_x() <= to_bounds.center_x() {
                // Left-to-right connection
                (
                    Point {
                        x: from_bounds.right(),
                        y: from_bounds.center_y(),
                    },
                    Point {
                        x: to_bounds.x,
                        y: to_bounds.center_y(),
                    },
                )
            } else {
                // Right-to-left connection
                (
                    Point {
                        x: from_bounds.x,
                        y: from_bounds.center_y(),
                    },
                    Point {
                        x: to_bounds.right(),
                        y: to_bounds.center_y(),
                    },
                )
            }
        } else {
            if from_bounds.center_y() <= to_bounds.center_y() {
                // Top-to-bottom connection
                (
                    Point {
                        x: from_bounds.center_x(),
                        y: from_bounds.bottom(),
                    },
                    Point {
                        x: to_bounds.center_x(),
                        y: to_bounds.y,
                    },
                )
            } else {
                // Bottom-to-top connection
                (
                    Point {
                        x: from_bounds.center_x(),
                        y: from_bounds.y,
                    },
                    Point {
                        x: to_bounds.center_x(),
                        y: to_bounds.bottom(),
                    },
                )
            }
        };

        let mid_x = (start.x + end.x) / 2.0;
        let mid_y = (start.y + end.y) / 2.0;

        let (control1, control2) = if is_horizontal {
            (Point { x: mid_x, y: start.y }, Point { x: mid_x, y: end.y })
        } else {
            (Point { x: start.x, y: mid_y }, Point { x: end.x, y: mid_y })
        };

        let label_pos = Point { x: mid_x, y: mid_y };

        positioned_edges.push(PositionedEdge {
            from_id: edge.from.clone(),
            to_id: edge.to.clone(),
            start,
            end,
            control1,
            control2,
            label: edge.label.clone(),
            label_pos,
            style: edge.style,
        });
    }

    // Step 4: Subgraphs Bounding Boxes
    let mut positioned_subgraphs = Vec::new();
    for sub in &diagram.subgraphs {
        let mut min_sub_x = f32::MAX;
        let mut min_sub_y = f32::MAX;
        let mut max_sub_x = f32::MIN;
        let mut max_sub_y = f32::MIN;
        let mut count = 0;

        for node_id in &sub.node_ids {
            if let Some(&bounds) = positioned_nodes_map.get(node_id) {
                min_sub_x = min_sub_x.min(bounds.x);
                min_sub_y = min_sub_y.min(bounds.y);
                max_sub_x = max_sub_x.max(bounds.right());
                max_sub_y = max_sub_y.max(bounds.bottom());
                count += 1;
            }
        }

        if count > 0 {
            let pad = 16.0;
            let bounds = Rect {
                x: min_sub_x - pad,
                y: min_sub_y - pad - 24.0, // space for subgraph title
                width: (max_sub_x - min_sub_x) + pad * 2.0,
                height: (max_sub_y - min_sub_y) + pad * 2.0 + 24.0,
            };
            positioned_subgraphs.push(PositionedSubgraph {
                id: sub.id.clone(),
                title: sub.title.clone(),
                bounds,
            });
            max_x = max_x.max(bounds.right());
            max_y = max_y.max(bounds.bottom());
        }
    }

    FlowchartLayout {
        canvas_width: max_x + PAD,
        canvas_height: max_y + PAD,
        nodes: positioned_nodes,
        edges: positioned_edges,
        subgraphs: positioned_subgraphs,
    }
}

fn layout_sequence(diagram: &SequenceDiagram) -> SequenceLayout {
    if diagram.participants.is_empty() {
        return SequenceLayout::default();
    }

    let col_w = 160.0f32;
    let mut participants = Vec::new();

    for (ix, p) in diagram.participants.iter().enumerate() {
        let x = PAD + (ix as f32) * (col_w + SIBLING_GAP);
        let header_bounds = Rect {
            x,
            y: PAD,
            width: col_w,
            height: NODE_H,
        };
        let lifeline_x = header_bounds.center_x();
        participants.push(PositionedParticipant {
            id: p.id.clone(),
            label: p.label.clone(),
            header_bounds,
            lifeline_x,
            is_actor: p.is_actor,
        });
    }

    let row_h = 42.0f32;
    let mut cur_y = PAD + NODE_H + 24.0;
    let mut messages = Vec::new();
    let mut notes = Vec::new();

    let get_lifeline = |id: &str, parts: &[PositionedParticipant]| -> Option<f32> {
        parts.iter().find(|p| p.id == id).map(|p| p.lifeline_x)
    };

    for (ix, msg) in diagram.messages.iter().enumerate() {
        let from_x = get_lifeline(&msg.from, &participants).unwrap_or(PAD + col_w / 2.0);
        let to_x = get_lifeline(&msg.to, &participants).unwrap_or(PAD + col_w * 1.5);

        messages.push(PositionedMessage {
            start: Point { x: from_x, y: cur_y },
            end: Point { x: to_x, y: cur_y },
            text: msg.text.clone(),
            is_dotted: msg.is_dotted,
            is_arrow: msg.is_arrow,
            is_cross: msg.is_cross,
            number: if diagram.auto_number { Some(ix + 1) } else { None },
        });

        cur_y += row_h;
    }

    for note in &diagram.notes {
        let (min_x, max_x) = if note.participants.is_empty() {
            (PAD, PAD + col_w)
        } else {
            let mut min_x = f32::MAX;
            let mut max_x = f32::MIN;
            for p_id in &note.participants {
                if let Some(lx) = get_lifeline(p_id, &participants) {
                    min_x = min_x.min(lx);
                    max_x = max_x.max(lx);
                }
            }
            (min_x, max_x)
        };

        let note_w = ((max_x - min_x) + 80.0).max(120.0);
        let note_x = min_x + (max_x - min_x) / 2.0 - note_w / 2.0;

        notes.push(PositionedNote {
            bounds: Rect {
                x: note_x,
                y: cur_y,
                width: note_w,
                height: 32.0,
            },
            text: note.text.clone(),
        });
        cur_y += 40.0;
    }

    let canvas_width = PAD + (diagram.participants.len() as f32) * (col_w + SIBLING_GAP) + PAD;
    let canvas_height = cur_y + 24.0;

    SequenceLayout {
        canvas_width,
        canvas_height,
        participants,
        messages,
        notes,
    }
}

fn layout_pie(diagram: &PieDiagram) -> PieLayout {
    let total: f64 = diagram.slices.iter().map(|s| s.value).sum();
    let mut slices = Vec::new();

    for (ix, s) in diagram.slices.iter().enumerate() {
        let pct = if total > 0.0 {
            (s.value / total) * 100.0
        } else {
            0.0
        };
        slices.push(PositionedPieSlice {
            label: s.label.clone(),
            value: s.value,
            percentage: pct,
            color_index: ix,
        });
    }

    PieLayout {
        title: diagram.title.clone(),
        total,
        slices,
    }
}

fn layout_state_as_flowchart(diagram: &StateDiagram) -> FlowchartLayout {
    let mut flowchart = FlowchartDiagram {
        direction: Direction::TopDown,
        ..Default::default()
    };

    for (id, state) in &diagram.states {
        flowchart.nodes.insert(
            id.clone(),
            MermaidNode {
                id: id.clone(),
                label: state.label.clone(),
                shape: NodeShape::Rounded,
                subgraph_id: None,
            },
        );
        flowchart.node_order.push(id.clone());
    }

    for trans in &diagram.transitions {
        flowchart.edges.push(MermaidEdge {
            from: trans.from.clone(),
            to: trans.to.clone(),
            label: trans.event.clone(),
            style: EdgeStyle::SolidArrow,
        });
    }

    layout_flowchart(&flowchart)
}

fn layout_class_as_flowchart(diagram: &ClassDiagram) -> FlowchartLayout {
    let mut flowchart = FlowchartDiagram {
        direction: Direction::TopDown,
        ..Default::default()
    };

    for (name, class_node) in &diagram.classes {
        let mut label = class_node.name.clone();
        if !class_node.attributes.is_empty() {
            label.push_str(&format!(" ({})", class_node.attributes.join(", ")));
        }
        flowchart.nodes.insert(
            name.clone(),
            MermaidNode {
                id: name.clone(),
                label,
                shape: NodeShape::Rectangle,
                subgraph_id: None,
            },
        );
        flowchart.node_order.push(name.clone());
    }

    for rel in &diagram.relations {
        flowchart.edges.push(MermaidEdge {
            from: rel.from.clone(),
            to: rel.to.clone(),
            label: rel.label.clone(),
            style: EdgeStyle::SolidArrow,
        });
    }

    layout_flowchart(&flowchart)
}
