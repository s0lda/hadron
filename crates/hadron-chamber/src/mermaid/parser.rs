//! Parser for Mermaid diagram definitions.

use super::ast::*;
use std::collections::HashMap;

/// Parse Mermaid source code into a structured diagram AST.
pub fn parse_mermaid(source: &str) -> Result<MermaidDiagram, String> {
    let clean_lines = preprocess_lines(source);
    if clean_lines.is_empty() {
        return Err("Empty mermaid diagram".to_string());
    }

    let first_line = clean_lines[0].trim();
    let header_lower = first_line.to_lowercase();

    if header_lower.starts_with("graph") || header_lower.starts_with("flowchart") {
        parse_flowchart(&clean_lines)
    } else if header_lower.starts_with("sequencediagram") {
        parse_sequence(&clean_lines)
    } else if header_lower.starts_with("pie") {
        parse_pie(&clean_lines)
    } else if header_lower.starts_with("statediagram") {
        parse_state(&clean_lines)
    } else if header_lower.starts_with("classdiagram") {
        parse_class(&clean_lines)
    } else {
        // Fallback to raw generic diagram type
        let diagram_type = first_line.split_whitespace().next().unwrap_or("mermaid").to_string();
        Ok(MermaidDiagram::Raw {
            diagram_type,
            source: source.to_string(),
        })
    }
}

fn preprocess_lines(source: &str) -> Vec<String> {
    source
        .lines()
        .map(|line| {
            let trimmed = line.trim();
            if let Some(pos) = trimmed.find("%%") {
                trimmed[..pos].trim().to_string()
            } else {
                trimmed.to_string()
            }
        })
        .filter(|line| !line.is_empty())
        .collect()
}

fn parse_flowchart(lines: &[String]) -> Result<MermaidDiagram, String> {
    let first_line = &lines[0];
    let mut parts = first_line.split_whitespace();
    let _header = parts.next();
    let dir_token = parts.next().unwrap_or("TD").to_uppercase();

    let direction = match dir_token.as_str() {
        "LR" => Direction::LeftRight,
        "RL" => Direction::RightLeft,
        "BT" => Direction::BottomTop,
        "TD" | "TB" | _ => Direction::TopDown,
    };

    let mut nodes: HashMap<String, MermaidNode> = HashMap::new();
    let mut node_order = Vec::new();
    let mut edges: Vec<MermaidEdge> = Vec::new();
    let mut subgraphs: Vec<MermaidSubgraph> = Vec::new();
    let mut active_subgraph: Option<String> = None;

    for line in &lines[1..] {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        // Subgraph start: `subgraph title` or `subgraph id [title]`
        if line.to_lowercase().starts_with("subgraph") {
            let rest = line[8..].trim();
            let (sub_id, title) = parse_subgraph_header(rest, subgraphs.len());
            active_subgraph = Some(sub_id.clone());
            subgraphs.push(MermaidSubgraph {
                id: sub_id,
                title,
                node_ids: Vec::new(),
            });
            continue;
        }

        if line.eq_ignore_ascii_case("end") {
            active_subgraph = None;
            continue;
        }

        // Directives like `classDef`, `style`, `click` can be ignored safely
        if line.starts_with("classDef")
            || line.starts_with("style")
            || line.starts_with("class ")
            || line.starts_with("click ")
            || line.starts_with("linkStyle")
        {
            continue;
        }

        // Parse line for edges or standalone nodes
        if let Some(parsed_edges) = parse_flowchart_line(
            line,
            &mut nodes,
            &mut node_order,
            active_subgraph.as_deref(),
        ) {
            edges.extend(parsed_edges);
        }
    }

    // Attach nodes to subgraphs
    for (id, node) in &nodes {
        if let Some(sub_id) = &node.subgraph_id {
            if let Some(sub) = subgraphs.iter_mut().find(|s| &s.id == sub_id) {
                if !sub.node_ids.contains(id) {
                    sub.node_ids.push(id.clone());
                }
            }
        }
    }

    Ok(MermaidDiagram::Flowchart(FlowchartDiagram {
        direction,
        nodes,
        node_order,
        edges,
        subgraphs,
    }))
}

fn parse_subgraph_header(rest: &str, count: usize) -> (String, String) {
    if let Some(idx) = rest.find('[') {
        let id = rest[..idx].trim().to_string();
        let title_part = &rest[idx..];
        let title = trim_brackets_and_quotes(title_part);
        (if id.is_empty() { format!("sub_{count}") } else { id }, title)
    } else {
        let title = trim_brackets_and_quotes(rest);
        (format!("sub_{count}"), title)
    }
}

/// Tokenize and parse node shapes and labels
pub fn parse_node_definition(input: &str) -> (String, Option<(String, NodeShape)>) {
    let input = input.trim();
    if input.is_empty() {
        return (String::new(), None);
    }

    // Match bracket patterns from longest to shortest
    // 1. Triple parens: (((text))) - DoubleCircle
    if let Some(start) = input.find("(((") {
        if let Some(end) = input.rfind(")))") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 3..end].trim();
                return (id, Some((unquote(label), NodeShape::DoubleCircle)));
            }
        }
    }

    // 2. Stadium: ([text])
    if let Some(start) = input.find("([") {
        if let Some(end) = input.rfind("])") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 2..end].trim();
                return (id, Some((unquote(label), NodeShape::Stadium)));
            }
        }
    }

    // 3. Subroutine: [[text]]
    if let Some(start) = input.find("[[") {
        if let Some(end) = input.rfind("]]") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 2..end].trim();
                return (id, Some((unquote(label), NodeShape::Subroutine)));
            }
        }
    }

    // 4. Cylinder: [(text)]
    if let Some(start) = input.find("[(") {
        if let Some(end) = input.rfind(")]") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 2..end].trim();
                return (id, Some((unquote(label), NodeShape::Cylinder)));
            }
        }
    }

    // 5. Circle: ((text))
    if let Some(start) = input.find("((") {
        if let Some(end) = input.rfind("))") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 2..end].trim();
                return (id, Some((unquote(label), NodeShape::Circle)));
            }
        }
    }

    // 6. Hexagon: {{text}}
    if let Some(start) = input.find("{{") {
        if let Some(end) = input.rfind("}}") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 2..end].trim();
                return (id, Some((unquote(label), NodeShape::Hexagon)));
            }
        }
    }

    // 7. Parallelogram: [/text/] or [\text\]
    if let Some(start) = input.find("[/") {
        if let Some(end) = input.rfind("/]") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 2..end].trim();
                return (id, Some((unquote(label), NodeShape::Parallelogram)));
            }
        }
    }

    // 8. Trapezoid: [/text\]
    if let Some(start) = input.find("[/") {
        if let Some(end) = input.rfind("\\]") {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 2..end].trim();
                return (id, Some((unquote(label), NodeShape::Trapezoid)));
            }
        }
    }

    // 9. Rectangle: [text]
    if let Some(start) = input.find('[') {
        if let Some(end) = input.rfind(']') {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 1..end].trim();
                return (id, Some((unquote(label), NodeShape::Rectangle)));
            }
        }
    }

    // 10. Diamond: {text}
    if let Some(start) = input.find('{') {
        if let Some(end) = input.rfind('}') {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 1..end].trim();
                return (id, Some((unquote(label), NodeShape::Diamond)));
            }
        }
    }

    // 11. Rounded: (text)
    if let Some(start) = input.find('(') {
        if let Some(end) = input.rfind(')') {
            if end > start {
                let id = input[..start].trim().to_string();
                let label = input[start + 1..end].trim();
                return (id, Some((unquote(label), NodeShape::Rounded)));
            }
        }
    }

    // Plain ID without brackets
    let plain = input.trim_matches(';').trim();
    (plain.to_string(), None)
}

fn unquote(s: &str) -> String {
    let s = s.trim();
    if (s.starts_with('"') && s.ends_with('"')) || (s.starts_with('\'') && s.ends_with('\'')) {
        if s.len() >= 2 {
            return s[1..s.len() - 1].to_string();
        }
    }
    s.to_string()
}

fn trim_brackets_and_quotes(s: &str) -> String {
    let s = s.trim();
    let s = s.trim_matches(|c| c == '[' || c == ']' || c == '"' || c == '\'');
    s.trim().to_string()
}

/// Register a node in the map if new, or update its label/shape if provided.
fn register_node(
    raw_token: &str,
    nodes: &mut HashMap<String, MermaidNode>,
    node_order: &mut Vec<String>,
    active_subgraph: Option<&str>,
) -> Vec<String> {
    let mut ids = Vec::new();

    // Handle `A & B` syntax
    for part in raw_token.split('&') {
        let (id, shape_info) = parse_node_definition(part.trim());
        let id = id.trim().to_string();
        if id.is_empty() {
            continue;
        }

        if let Some((label, shape)) = shape_info {
            nodes.insert(
                id.clone(),
                MermaidNode {
                    id: id.clone(),
                    label,
                    shape,
                    subgraph_id: active_subgraph.map(str::to_string),
                },
            );
        } else if !nodes.contains_key(&id) {
            nodes.insert(
                id.clone(),
                MermaidNode {
                    id: id.clone(),
                    label: id.clone(),
                    shape: NodeShape::Rectangle,
                    subgraph_id: active_subgraph.map(str::to_string),
                },
            );
        }

        if !node_order.contains(&id) {
            node_order.push(id.clone());
        }
        ids.push(id);
    }

    ids
}

/// Edge connector pattern signatures
struct EdgePattern {
    token: &'static str,
    style: EdgeStyle,
}

const EDGE_PATTERNS: &[EdgePattern] = &[
    EdgePattern { token: "==>", style: EdgeStyle::ThickArrow },
    EdgePattern { token: "===", style: EdgeStyle::ThickLine },
    EdgePattern { token: "-.->", style: EdgeStyle::DottedArrow },
    EdgePattern { token: "-.-", style: EdgeStyle::DottedLine },
    EdgePattern { token: "-->", style: EdgeStyle::SolidArrow },
    EdgePattern { token: "---", style: EdgeStyle::SolidLine },
    EdgePattern { token: "->", style: EdgeStyle::SolidArrow },
    EdgePattern { token: "--", style: EdgeStyle::SolidLine },
];

fn parse_flowchart_line(
    line: &str,
    nodes: &mut HashMap<String, MermaidNode>,
    node_order: &mut Vec<String>,
    active_subgraph: Option<&str>,
) -> Option<Vec<MermaidEdge>> {
    let line = line.trim_end_matches(';').trim();
    if line.is_empty() {
        return None;
    }

    // Check if line contains any edge connector
    let mut has_connector = false;
    for pat in EDGE_PATTERNS {
        if line.contains(pat.token) {
            has_connector = true;
            break;
        }
    }

    if !has_connector {
        // Standalone node definition
        register_node(line, nodes, node_order, active_subgraph);
        return None;
    }

    // Split chain into tokens and edges
    let mut collected_edges = Vec::new();
    let mut current_sources = Vec::new();
    let mut remainder = line;

    while !remainder.is_empty() {
        // Find earliest edge pattern
        let mut earliest_match: Option<(usize, usize, EdgeStyle, Option<String>)> = None;

        // First check for `-->|label|` or `-- label -->`
        for pat in EDGE_PATTERNS {
            if let Some(pos) = remainder.find(pat.token) {
                let token_len = pat.token.len();
                let after = &remainder[pos + token_len..];
                
                // Case 1: `-->|label| Target`
                if after.starts_with('|') {
                    if let Some(end_bar) = after[1..].find('|') {
                        let label = after[1..1 + end_bar].trim().to_string();
                        let total_len = token_len + 1 + end_bar + 1;
                        if earliest_match.as_ref().map_or(true, |(p, ..)| pos < *p) {
                            earliest_match = Some((pos, total_len, pat.style, Some(label)));
                        }
                        continue;
                    }
                }

                // Case 2: `-- label --> Target`
                if pat.token == "--" {
                    if let Some(arrow_pos) = after.find("-->") {
                        let label = after[..arrow_pos].trim().to_string();
                        let total_len = token_len + arrow_pos + 3;
                        if earliest_match.as_ref().map_or(true, |(p, ..)| pos < *p) {
                            earliest_match = Some((pos, total_len, EdgeStyle::SolidArrow, Some(label)));
                        }
                        continue;
                    }
                    if let Some(dash_pos) = after.find("---") {
                        let label = after[..dash_pos].trim().to_string();
                        let total_len = token_len + dash_pos + 3;
                        if earliest_match.as_ref().map_or(true, |(p, ..)| pos < *p) {
                            earliest_match = Some((pos, total_len, EdgeStyle::SolidLine, Some(label)));
                        }
                        continue;
                    }
                }

                // Standard edge without inline label
                if earliest_match.as_ref().map_or(true, |(p, ..)| pos < *p) {
                    earliest_match = Some((pos, token_len, pat.style, None));
                }
            }
        }

        if let Some((pos, match_len, style, label)) = earliest_match {
            let left_token = &remainder[..pos].trim();
            if !left_token.is_empty() {
                current_sources = register_node(left_token, nodes, node_order, active_subgraph);
            }

            let next_segment = &remainder[pos + match_len..].trim();
            remainder = next_segment;

            // Peek next target token (up to next edge pattern or end of string)
            let mut next_edge_pos = remainder.len();
            for pat in EDGE_PATTERNS {
                if let Some(p) = remainder.find(pat.token) {
                    next_edge_pos = next_edge_pos.min(p);
                }
            }

            let target_token = remainder[..next_edge_pos].trim();
            let target_ids = register_node(target_token, nodes, node_order, active_subgraph);

            for src in &current_sources {
                for tgt in &target_ids {
                    collected_edges.push(MermaidEdge {
                        from: src.clone(),
                        to: tgt.clone(),
                        label: label.clone(),
                        style,
                    });
                }
            }

            current_sources = target_ids;
            remainder = &remainder[next_edge_pos..];
        } else {
            if !remainder.trim().is_empty() {
                register_node(remainder.trim(), nodes, node_order, active_subgraph);
            }
            break;
        }
    }

    Some(collected_edges)
}

fn parse_sequence(lines: &[String]) -> Result<MermaidDiagram, String> {
    let mut auto_number = false;
    let mut participants: Vec<SequenceParticipant> = Vec::new();
    let mut messages: Vec<SequenceMessage> = Vec::new();
    let mut notes: Vec<SequenceNote> = Vec::new();

    let ensure_participant = |id: &str, is_actor: bool, parts: &mut Vec<SequenceParticipant>| {
        let id = id.trim().to_string();
        if !id.is_empty() && !parts.iter().any(|p| p.id == id) {
            parts.push(SequenceParticipant {
                id: id.clone(),
                label: id,
                is_actor,
            });
        }
    };

    for line in &lines[1..] {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.eq_ignore_ascii_case("autonumber") {
            auto_number = true;
            continue;
        }

        if line.starts_with("participant ") || line.starts_with("actor ") {
            let is_actor = line.starts_with("actor ");
            let rest = if is_actor { &line[6..] } else { &line[12..] }.trim();
            if let Some(as_pos) = rest.find(" as ") {
                let id = rest[..as_pos].trim().to_string();
                let label = unquote(rest[as_pos + 4..].trim());
                participants.push(SequenceParticipant {
                    id,
                    label,
                    is_actor,
                });
            } else {
                let id = unquote(rest);
                participants.push(SequenceParticipant {
                    id: id.clone(),
                    label: id,
                    is_actor,
                });
            }
            continue;
        }

        // Note [over|right of|left of]
        if line.to_lowercase().starts_with("note ") {
            let rest = line[5..].trim();
            let (placement, p_part, text) = if let Some(colon) = rest.find(':') {
                let head = rest[..colon].trim();
                let text = rest[colon + 1..].trim().to_string();
                let (pl, part_names) = if head.to_lowercase().starts_with("over ") {
                    (NotePlacement::Over, &head[5..])
                } else if head.to_lowercase().starts_with("right of ") {
                    (NotePlacement::RightOf, &head[9..])
                } else if head.to_lowercase().starts_with("left of ") {
                    (NotePlacement::LeftOf, &head[8..])
                } else {
                    (NotePlacement::Over, head)
                };
                let parts: Vec<String> = part_names.split(',').map(|s| s.trim().to_string()).collect();
                (pl, parts, text)
            } else {
                (NotePlacement::Over, vec![], rest.to_string())
            };
            for p in &p_part {
                ensure_participant(p, false, &mut participants);
            }
            notes.push(SequenceNote {
                participants: p_part,
                placement,
                text,
            });
            continue;
        }

        // Sequence messages: `A->>B: text`, `A-->>B: text`, `A->B: text`, `A-->B: text`, `A-xB: text`, `A--xB: text`
        const SEQ_PATTERNS: &[(&str, bool, bool, bool)] = &[
            ("-->>", true, true, false),
            ("->>", false, true, false),
            ("--x", true, true, true),
            ("-x", false, true, true),
            ("-->", true, false, false),
            ("->", false, false, false),
        ];

        for (pat, is_dotted, is_arrow, is_cross) in SEQ_PATTERNS {
            if let Some(pos) = line.find(pat) {
                let from = line[..pos].trim_end_matches(['+', '-']).trim();
                let rest = &line[pos + pat.len()..];
                let (to, text) = if let Some(colon) = rest.find(':') {
                    (
                        rest[..colon].trim_end_matches(['+', '-']).trim(),
                        rest[colon + 1..].trim(),
                    )
                } else {
                    (rest.trim_end_matches(['+', '-']).trim(), "")
                };

                ensure_participant(from, false, &mut participants);
                ensure_participant(to, false, &mut participants);

                messages.push(SequenceMessage {
                    from: from.to_string(),
                    to: to.to_string(),
                    text: text.to_string(),
                    is_dotted: *is_dotted,
                    is_arrow: *is_arrow,
                    is_cross: *is_cross,
                });
                break;
            }
        }
    }

    Ok(MermaidDiagram::Sequence(SequenceDiagram {
        auto_number,
        participants,
        messages,
        notes,
    }))
}

fn parse_pie(lines: &[String]) -> Result<MermaidDiagram, String> {
    let mut title = None;
    let mut show_data = false;
    let mut slices = Vec::new();

    let first_line = &lines[0];
    if first_line.to_lowercase().contains("showdata") {
        show_data = true;
    }
    if let Some(idx) = first_line.to_lowercase().find("title ") {
        title = Some(first_line[idx + 6..].trim().to_string());
    }

    for line in &lines[1..] {
        let line = line.trim();
        if line.is_empty() {
            continue;
        }

        if line.to_lowercase().starts_with("title ") {
            title = Some(line[6..].trim().to_string());
            continue;
        }

        if line.eq_ignore_ascii_case("showData") {
            show_data = true;
            continue;
        }

        if let Some(colon) = line.find(':') {
            let label = unquote(line[..colon].trim());
            let value_str = line[colon + 1..].trim();
            if let Ok(value) = value_str.parse::<f64>() {
                slices.push(PieSlice { label, value });
            }
        }
    }

    Ok(MermaidDiagram::Pie(PieDiagram {
        title,
        show_data,
        slices,
    }))
}

fn parse_state(lines: &[String]) -> Result<MermaidDiagram, String> {
    let mut states: HashMap<String, StateNode> = HashMap::new();
    let mut transitions: Vec<StateTransition> = Vec::new();

    for line in &lines[1..] {
        let line = line.trim_end_matches(';').trim();
        if line.is_empty() {
            continue;
        }

        if let Some(pos) = line.find("-->") {
            let from = line[..pos].trim();
            let rest = &line[pos + 3..];
            let (to, event) = if let Some(colon) = rest.find(':') {
                (rest[..colon].trim(), Some(rest[colon + 1..].trim().to_string()))
            } else {
                (rest.trim(), None)
            };

            let is_start = from == "[*]";
            let is_end = to == "[*]";

            if !is_start && !states.contains_key(from) {
                states.insert(from.to_string(), StateNode {
                    id: from.to_string(),
                    label: from.to_string(),
                    is_start: false,
                    is_end: false,
                });
            }

            if !is_end && !states.contains_key(to) {
                states.insert(to.to_string(), StateNode {
                    id: to.to_string(),
                    label: to.to_string(),
                    is_start: false,
                    is_end: false,
                });
            }

            transitions.push(StateTransition {
                from: from.to_string(),
                to: to.to_string(),
                event,
            });
        }
    }

    Ok(MermaidDiagram::State(StateDiagram {
        states,
        transitions,
    }))
}

fn parse_class(lines: &[String]) -> Result<MermaidDiagram, String> {
    let mut classes: HashMap<String, ClassNode> = HashMap::new();
    let mut relations: Vec<ClassRelation> = Vec::new();

    for line in &lines[1..] {
        let line = line.trim_end_matches(';').trim();
        if line.is_empty() {
            continue;
        }

        const CLASS_RELS: &[&str] = &["<|--", "*--", "o--", "-->", "--", "..>", "<|.."];
        let mut matched_rel = None;
        for rel in CLASS_RELS {
            if let Some(pos) = line.find(rel) {
                matched_rel = Some((pos, *rel));
                break;
            }
        }

        if let Some((pos, rel)) = matched_rel {
            let from = line[..pos].trim();
            let to = &line[pos + rel.len()..];
            let (to_name, label) = if let Some(colon) = to.find(':') {
                (to[..colon].trim(), Some(to[colon + 1..].trim().to_string()))
            } else {
                (to.trim(), None)
            };

            if !classes.contains_key(from) {
                classes.insert(from.to_string(), ClassNode {
                    name: from.to_string(),
                    attributes: Vec::new(),
                    methods: Vec::new(),
                });
            }
            if !classes.contains_key(to_name) {
                classes.insert(to_name.to_string(), ClassNode {
                    name: to_name.to_string(),
                    attributes: Vec::new(),
                    methods: Vec::new(),
                });
            }

            relations.push(ClassRelation {
                from: from.to_string(),
                to: to_name.to_string(),
                relation_type: rel.to_string(),
                label,
            });
        } else if line.starts_with("class ") {
            let name = line[6..].trim().trim_end_matches('{').trim();
            if !classes.contains_key(name) {
                classes.insert(name.to_string(), ClassNode {
                    name: name.to_string(),
                    attributes: Vec::new(),
                    methods: Vec::new(),
                });
            }
        }
    }

    Ok(MermaidDiagram::Class(ClassDiagram { classes, relations }))
}
