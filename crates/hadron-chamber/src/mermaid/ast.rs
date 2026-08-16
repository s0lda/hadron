//! Abstract Syntax Tree definitions for parsed Mermaid diagrams.

use std::collections::HashMap;

/// Supported high-level Mermaid diagram types.
#[derive(Debug, Clone, PartialEq)]
pub enum MermaidDiagram {
    Flowchart(FlowchartDiagram),
    Sequence(SequenceDiagram),
    Pie(PieDiagram),
    State(StateDiagram),
    Class(ClassDiagram),
    Raw {
        diagram_type: String,
        source: String,
    },
}

impl MermaidDiagram {
    /// Friendly display title of this diagram type.
    pub fn title(&self) -> &str {
        match self {
            MermaidDiagram::Flowchart(f) => match f.direction {
                Direction::TopDown => "Flowchart (TD)",
                Direction::LeftRight => "Flowchart (LR)",
                Direction::BottomTop => "Flowchart (BT)",
                Direction::RightLeft => "Flowchart (RL)",
            },
            MermaidDiagram::Sequence(_) => "Sequence Diagram",
            MermaidDiagram::Pie(_) => "Pie Chart",
            MermaidDiagram::State(_) => "State Diagram",
            MermaidDiagram::Class(_) => "Class Diagram",
            MermaidDiagram::Raw { diagram_type, .. } => diagram_type.as_str(),
        }
    }

    /// Summary metrics string, e.g. "7 nodes · 3 edges"
    pub fn metrics_summary(&self) -> String {
        match self {
            MermaidDiagram::Flowchart(f) => {
                format!("{} nodes · {} edges", f.nodes.len(), f.edges.len())
            }
            MermaidDiagram::Sequence(s) => {
                format!(
                    "{} participants · {} messages",
                    s.participants.len(),
                    s.messages.len()
                )
            }
            MermaidDiagram::Pie(p) => {
                format!("{} slices", p.slices.len())
            }
            MermaidDiagram::State(s) => {
                format!("{} states · {} transitions", s.states.len(), s.transitions.len())
            }
            MermaidDiagram::Class(c) => {
                format!("{} classes · {} relations", c.classes.len(), c.relations.len())
            }
            MermaidDiagram::Raw { .. } => "Mermaid Diagram".to_string(),
        }
    }
}

/// Flowchart orientation direction.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Direction {
    TopDown,
    LeftRight,
    BottomTop,
    RightLeft,
}

impl Default for Direction {
    fn default() -> Self {
        Direction::TopDown
    }
}

/// A parsed Flowchart/Graph diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct FlowchartDiagram {
    pub direction: Direction,
    pub nodes: HashMap<String, MermaidNode>,
    pub node_order: Vec<String>,
    pub edges: Vec<MermaidEdge>,
    pub subgraphs: Vec<MermaidSubgraph>,
}

/// Node representation in a flowchart.
#[derive(Debug, Clone, PartialEq)]
pub struct MermaidNode {
    pub id: String,
    pub label: String,
    pub shape: NodeShape,
    pub subgraph_id: Option<String>,
}

/// Visual shape of a flowchart node.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeShape {
    Rectangle,    // [text]
    Rounded,      // (text)
    Stadium,      // ([text])
    Subroutine,   // [[text]]
    Cylinder,     // [(text)]
    Circle,       // ((text))
    DoubleCircle, // (((text)))
    Diamond,      // {text}
    Hexagon,      // {{text}}
    Parallelogram,// [/text/] or [\text\]
    Trapezoid,    // [/text\] or [\text/]
}

impl Default for NodeShape {
    fn default() -> Self {
        NodeShape::Rectangle
    }
}

/// Connection between two flowchart nodes.
#[derive(Debug, Clone, PartialEq)]
pub struct MermaidEdge {
    pub from: String,
    pub to: String,
    pub label: Option<String>,
    pub style: EdgeStyle,
}

/// Styling of a connector line.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeStyle {
    SolidArrow, // -->
    SolidLine,  // ---
    DottedArrow,// -.->
    DottedLine, // -.-
    ThickArrow, // ==>
    ThickLine,  // ===
}

impl Default for EdgeStyle {
    fn default() -> Self {
        EdgeStyle::SolidArrow
    }
}

/// Grouping subgraph container in a flowchart.
#[derive(Debug, Clone, PartialEq)]
pub struct MermaidSubgraph {
    pub id: String,
    pub title: String,
    pub node_ids: Vec<String>,
}

/// Parsed Sequence diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct SequenceDiagram {
    pub auto_number: bool,
    pub participants: Vec<SequenceParticipant>,
    pub messages: Vec<SequenceMessage>,
    pub notes: Vec<SequenceNote>,
}

/// Participant or Actor in a sequence diagram.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceParticipant {
    pub id: String,
    pub label: String,
    pub is_actor: bool,
}

/// Message exchange between sequence participants.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceMessage {
    pub from: String,
    pub to: String,
    pub text: String,
    pub is_dotted: bool,
    pub is_arrow: bool,
    pub is_cross: bool,
}

/// Note attached to sequence lifelines.
#[derive(Debug, Clone, PartialEq)]
pub struct SequenceNote {
    pub participants: Vec<String>,
    pub placement: NotePlacement,
    pub text: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum NotePlacement {
    Over,
    LeftOf,
    RightOf,
}

/// Parsed Pie chart diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct PieDiagram {
    pub title: Option<String>,
    pub show_data: bool,
    pub slices: Vec<PieSlice>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct PieSlice {
    pub label: String,
    pub value: f64,
}

/// Parsed State diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct StateDiagram {
    pub states: HashMap<String, StateNode>,
    pub transitions: Vec<StateTransition>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateNode {
    pub id: String,
    pub label: String,
    pub is_start: bool,
    pub is_end: bool,
}

#[derive(Debug, Clone, PartialEq)]
pub struct StateTransition {
    pub from: String,
    pub to: String,
    pub event: Option<String>,
}

/// Parsed Class diagram.
#[derive(Debug, Clone, PartialEq, Default)]
pub struct ClassDiagram {
    pub classes: HashMap<String, ClassNode>,
    pub relations: Vec<ClassRelation>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassNode {
    pub name: String,
    pub attributes: Vec<String>,
    pub methods: Vec<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct ClassRelation {
    pub from: String,
    pub to: String,
    pub relation_type: String,
    pub label: Option<String>,
}
