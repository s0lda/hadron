//! Native Mermaid diagram parsing, layout, and GPUI rendering module.

pub mod ast;
pub mod parser;
pub mod layout;

#[cfg(feature = "gui")]
pub mod render;
#[cfg(feature = "gui")]
pub mod plugin;

#[cfg(test)]
mod tests {
    use super::ast::*;
    use super::layout::*;
    use super::parser::*;

    #[test]
    fn parses_flowchart_directions_and_shapes() {
        let src = r#"
        graph LR
          A[Rectangle] --> B(Rounded)
          B --> C([Stadium])
          C --> D[[Subroutine]]
          D --> E[(Cylinder)]
          E --> F((Circle))
          F --> G{Diamond}
          G --> H{{Hexagon}}
          H --> I[/Parallelogram/]
          I --> J[\Trapezoid/]
        "#;

        let diagram = parse_mermaid(src).expect("should parse flowchart");
        let MermaidDiagram::Flowchart(f) = diagram else {
            panic!("expected flowchart");
        };

        assert_eq!(f.direction, Direction::LeftRight);
        assert_eq!(f.nodes.len(), 10);
        assert_eq!(f.edges.len(), 9);
        assert_eq!(f.nodes.get("A").unwrap().shape, NodeShape::Rectangle);
        assert_eq!(f.nodes.get("B").unwrap().shape, NodeShape::Rounded);
        assert_eq!(f.nodes.get("C").unwrap().shape, NodeShape::Stadium);
        assert_eq!(f.nodes.get("D").unwrap().shape, NodeShape::Subroutine);
        assert_eq!(f.nodes.get("E").unwrap().shape, NodeShape::Cylinder);
        assert_eq!(f.nodes.get("F").unwrap().shape, NodeShape::Circle);
        assert_eq!(f.nodes.get("G").unwrap().shape, NodeShape::Diamond);
        assert_eq!(f.nodes.get("H").unwrap().shape, NodeShape::Hexagon);
        assert_eq!(f.nodes.get("I").unwrap().shape, NodeShape::Parallelogram);
    }

    #[test]
    fn parses_edge_styles_and_labels() {
        let src = r#"
        flowchart TD
          A -->|direct label| B
          B -- dotted label --> C
          C ==>|thick label| D
          D -.-> E
          E === F
        "#;

        let diagram = parse_mermaid(src).expect("should parse edges");
        let MermaidDiagram::Flowchart(f) = diagram else {
            panic!("expected flowchart");
        };

        assert_eq!(f.direction, Direction::TopDown);
        assert_eq!(f.edges.len(), 5);
        assert_eq!(f.edges[0].label.as_deref(), Some("direct label"));
        assert_eq!(f.edges[0].style, EdgeStyle::SolidArrow);
        assert_eq!(f.edges[1].label.as_deref(), Some("dotted label"));
        assert_eq!(f.edges[2].label.as_deref(), Some("thick label"));
        assert_eq!(f.edges[2].style, EdgeStyle::ThickArrow);
        assert_eq!(f.edges[3].style, EdgeStyle::DottedArrow);
        assert_eq!(f.edges[4].style, EdgeStyle::ThickLine);
    }

    #[test]
    fn parses_subgraphs_and_nested_nodes() {
        let src = r#"
        graph TB
          subgraph Core ["Engine Core"]
            A[Parser] --> B[AST]
          end
          subgraph UI ["Chamber UI"]
            C[Renderer] --> D[View]
          end
          B --> C
        "#;

        let diagram = parse_mermaid(src).expect("should parse subgraphs");
        let MermaidDiagram::Flowchart(f) = diagram else {
            panic!("expected flowchart");
        };

        assert_eq!(f.subgraphs.len(), 2);
        assert_eq!(f.subgraphs[0].title, "Engine Core");
        assert_eq!(f.subgraphs[1].title, "Chamber UI");
        assert_eq!(f.nodes.get("A").unwrap().subgraph_id.as_deref(), Some("Core"));
        assert_eq!(f.nodes.get("C").unwrap().subgraph_id.as_deref(), Some("UI"));
    }

    #[test]
    fn parses_live_hadron_nucleus_diagram() {
        let src = r#"
        graph TD
          compiled_is_not_running["compiled-is-not-running"]
          done_means_committed_not_working["done-means-committed-not-working"]
          the_prompt_can_be_wrong["the-prompt-can-be-wrong"]
          used_tokens_means_three_different_things["used_tokens-means-three-different-things"]
          acp_tokens_are_mostly_cache_reads["acp-tokens-are-mostly-cache-reads"]
          a_stated_budget_rule_does_not_hold_itself["a-stated-budget-rule-does-not-hold-itself"]
          the_nucleus_was_inert_over_budget["the-nucleus-was-inert-over-budget"]
          
          used_tokens_means_three_different_things --> the_depletion_gate_is_a_loaded_gun
          used_tokens_means_three_different_things --> acp_tokens_are_mostly_cache_reads
          a_stated_budget_rule_does_not_hold_itself --> the_nucleus_was_inert_over_budget
        "#;

        let diagram = parse_mermaid(src).expect("should parse nucleus diagram");
        let MermaidDiagram::Flowchart(ref f) = diagram else {
            panic!("expected flowchart");
        };

        assert_eq!(f.nodes.len(), 8);
        assert_eq!(f.edges.len(), 3);
        assert_eq!(
            f.nodes.get("compiled_is_not_running").unwrap().label,
            "compiled-is-not-running"
        );

        let layout = match compute_layout(&diagram) {
            LayoutResult::Flowchart(l) => l,
            _ => panic!("expected flowchart layout"),
        };

        assert!(layout.canvas_width > 0.0);
        assert!(layout.canvas_height > 0.0);
        assert_eq!(layout.nodes.len(), 8);
        assert_eq!(layout.edges.len(), 3);
    }

    #[test]
    fn parses_sequence_diagram() {
        let src = r#"
        sequenceDiagram
          autonumber
          actor Human
          participant Orchestrator as @Agy
          participant Worker as @Sonnet
          Human->>Orchestrator: Please implement feature
          Note over Orchestrator: Planning tasks
          Orchestrator->>Worker: Dispatch task 1
          Worker-->>Orchestrator: Completed in commit abc1234
          Orchestrator-->>Human: Feature live
        "#;

        let diagram = parse_mermaid(src).expect("should parse sequence diagram");
        let MermaidDiagram::Sequence(ref s) = diagram else {
            panic!("expected sequence diagram");
        };

        assert!(s.auto_number);
        assert_eq!(s.participants.len(), 3);
        assert_eq!(s.participants[0].id, "Human");
        assert!(s.participants[0].is_actor);
        assert_eq!(s.participants[1].label, "@Agy");
        assert_eq!(s.messages.len(), 4);
        assert_eq!(s.notes.len(), 1);

        let layout = match compute_layout(&diagram) {
            LayoutResult::Sequence(l) => l,
            _ => panic!("expected sequence layout"),
        };

        assert!(layout.canvas_width > 0.0);
        assert!(layout.canvas_height > 0.0);
        assert_eq!(layout.participants.len(), 3);
        assert_eq!(layout.messages.len(), 4);
        assert_eq!(layout.messages[0].number, Some(1));
    }

    #[test]
    fn parses_pie_chart() {
        let src = r#"
        pie showData title Crate LOC Distribution
          "hadron-chamber" : 12500
          "hadron-gluon" : 18300
          "hadron-forge" : 9400
          "hadron-lattice" : 6200
        "#;

        let diagram = parse_mermaid(src).expect("should parse pie chart");
        let MermaidDiagram::Pie(ref p) = diagram else {
            panic!("expected pie chart");
        };


        assert_eq!(p.title.as_deref(), Some("Crate LOC Distribution"));
        assert!(p.show_data);
        assert_eq!(p.slices.len(), 4);
        assert_eq!(p.slices[0].label, "hadron-chamber");
        assert_eq!(p.slices[0].value, 12500.0);

        let layout = match compute_layout(&diagram) {
            LayoutResult::Pie(l) => l,
            _ => panic!("expected pie layout"),
        };

        assert_eq!(layout.total, 46400.0);
        assert_eq!(layout.slices.len(), 4);
        let first_pct = layout.slices[0].percentage;
        assert!((first_pct - 26.939).abs() < 0.01);
    }

    #[test]
    fn parses_state_diagram() {
        let src = r#"
        stateDiagram-v2
          [*] --> Idle
          Idle --> Running: StartTurn
          Running --> Verifying: TestsPass
          Verifying --> Landed: MergeGateOk
          Landed --> [*]
        "#;

        let diagram = parse_mermaid(src).expect("should parse state diagram");
        let MermaidDiagram::State(s) = diagram else {
            panic!("expected state diagram");
        };

        assert_eq!(s.states.len(), 4);
        assert_eq!(s.transitions.len(), 5);
        assert_eq!(s.transitions[1].event.as_deref(), Some("StartTurn"));
    }

    #[test]
    fn parses_class_diagram() {
        let src = r#"
        classDiagram
          class Quark {
            +String id
            +String model
            +execute()
          }
          class Orchestrator
          Quark <|-- Orchestrator
        "#;

        let diagram = parse_mermaid(src).expect("should parse class diagram");
        let MermaidDiagram::Class(c) = diagram else {
            panic!("expected class diagram");
        };

        assert_eq!(c.classes.len(), 2);
        assert_eq!(c.relations.len(), 1);
        assert_eq!(c.relations[0].from, "Quark");
        assert_eq!(c.relations[0].to, "Orchestrator");
    }

    #[test]
    #[cfg(feature = "gui")]
    fn test_chamber_markdown_extensions_constructs_text_view() {
        let md = "Here is a diagram:\n\n```mermaid\ngraph LR\nA --> B\n```\n";
        let _tv = gpui_component::text::TextView::markdown("test-mermaid-view", md)
            .markdown_extensions(super::plugin::chamber_markdown_extensions());
    }

    #[test]
    #[cfg(feature = "gui")]
    fn test_mermaid_card_constructs() {
        let card = super::render::MermaidCard::new("graph TD\nA --> B\nB --> C");
        assert!(card.diagram.is_ok());
    }

    #[test]
    fn test_parse_shield_badges() {
        use super::plugin::parse_shield_badge;

        let b1 = parse_shield_badge("https://img.shields.io/badge/License-Apache_2.0-blue.svg").expect("parse license badge");
        assert_eq!(b1.label, "License");
        assert_eq!(b1.status, "Apache 2.0");
        assert_eq!(b1.color_name, "blue");

        let b2 = parse_shield_badge("https://img.shields.io/badge/Language-Rust_2021-orange.svg").expect("parse rust badge");
        assert_eq!(b2.label, "Language");
        assert_eq!(b2.status, "Rust 2021");
        assert_eq!(b2.color_name, "orange");

        let b3 = parse_shield_badge("https://img.shields.io/badge/Protocol-Agent_Client_Protocol_%28ACP%29-green.svg").expect("parse acp badge");
        assert_eq!(b3.label, "Protocol");
        assert_eq!(b3.status, "Agent Client Protocol (ACP)");
        assert_eq!(b3.color_name, "green");

        let b4 = parse_shield_badge("https://img.shields.io/badge/Architecture-Decoupled_Zero--CPU_Bus-red.svg").expect("parse arch badge");
        assert_eq!(b4.label, "Architecture");
        assert_eq!(b4.status, "Decoupled Zero-CPU Bus");
        assert_eq!(b4.color_name, "red");

        let b5 = parse_shield_badge("https://img.shields.io/static/v1?label=GUI&message=GPUI&color=purple").expect("parse static badge");
        assert_eq!(b5.label, "GUI");
        assert_eq!(b5.status, "GPUI");
        assert_eq!(b5.color_name, "purple");

        let b6 = parse_shield_badge("https://badgen.net/badge/License/Apache_2.0/blue").expect("parse badgen");
        assert_eq!(b6.label, "License");
        assert_eq!(b6.status, "Apache 2.0");
        assert_eq!(b6.color_name, "blue");
    }

    #[test]
    fn test_parse_html_img() {
        use super::plugin::parse_html_img;

        let img_html = r#"<img src="assets/demo_3.png" alt="Hadron Orchestrated Multi-Provider Chat Workspace" width="900" />"#;
        let data = parse_html_img(img_html).expect("should parse html img");
        assert_eq!(data.url, "assets/demo_3.png");
        assert_eq!(data.alt.as_deref(), Some("Hadron Orchestrated Multi-Provider Chat Workspace"));
        assert_eq!(data.width, Some(900.0));
    }

    #[test]
    fn test_resolve_image_path() {
        use super::plugin::resolve_image_path;
        let repo_root = std::path::Path::new("/home/Jake/dev/hadron");
        let resolved = resolve_image_path("assets/demo_3.png", repo_root);
        assert!(resolved.is_some(), "assets/demo_3.png should resolve to an existing file");
        assert!(resolved.unwrap().exists());
    }

    #[test]
    fn test_format_html_wrappers() {
        let md = r#"# 🌌 Hadron

<div align="center">

**The Native Desktop Workspace**

[![License](https://img.shields.io/badge/License-Apache_2.0-blue.svg)](LICENSE)

<br />

<img src="assets/demo_3.png" alt="Hadron Demo" width="900" />

</div>
"#;
        let unwrapped = super::plugin::format_html_wrappers(md);
        assert!(!unwrapped.contains("<div"));
        assert!(!unwrapped.contains("</div>"));
        assert!(!unwrapped.contains("<br"));
        assert!(unwrapped.contains("**The Native Desktop Workspace**"));
        assert!(unwrapped.contains("[![License]"));
        assert!(unwrapped.contains(r#"<img src="assets/demo_3.png""#));
    }

    #[test]
    fn test_format_bytes() {
        use super::plugin::format_bytes;
        assert_eq!(format_bytes(500), "500 B");
        assert_eq!(format_bytes(1024), "1 KB");
        assert_eq!(format_bytes(1024 * 500), "500 KB");
        assert_eq!(format_bytes(1024 * 1024 * 2), "2.0 MB");
    }

    #[test]
    fn test_probe_image_meta_and_dimensions() {
        use super::plugin::{probe_image_meta, probe_image_dimensions_from_bytes};

        let repo_root = std::path::Path::new("/home/Jake/dev/hadron");
        let demo3 = repo_root.join("assets/demo_3.png");
        if demo3.exists() {
            let (dims, size) = probe_image_meta(&demo3);
            assert_eq!(dims, Some((2184, 1199)), "demo_3.png dimensions should be 2184x1199");
            assert!(size.is_some() && size.unwrap() > 0);
        }

        let app_icon = repo_root.join("assets/hadron_app_icon.png");
        if app_icon.exists() {
            let (dims, _size) = probe_image_meta(&app_icon);
            assert_eq!(dims, Some((1024, 1024)), "hadron_app_icon.png dimensions should be 1024x1024");
        }

        // Test synthetic PNG bytes (8-byte sig + IHDR chunk)
        let mut png_bytes = vec![0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]; // sig
        png_bytes.extend_from_slice(&[0, 0, 0, 13]); // length
        png_bytes.extend_from_slice(b"IHDR"); // chunk type
        png_bytes.extend_from_slice(&800u32.to_be_bytes()); // width = 800
        png_bytes.extend_from_slice(&600u32.to_be_bytes()); // height = 600
        png_bytes.extend_from_slice(&[8, 6, 0, 0, 0]); // bit depth, color type, etc.
        assert_eq!(probe_image_dimensions_from_bytes(&png_bytes), Some((800, 600)));

        // Test synthetic GIF bytes
        let mut gif_bytes = Vec::from(b"GIF89a".as_slice());
        gif_bytes.extend_from_slice(&320u16.to_le_bytes()); // width = 320
        gif_bytes.extend_from_slice(&240u16.to_le_bytes()); // height = 240
        assert_eq!(probe_image_dimensions_from_bytes(&gif_bytes), Some((320, 240)));
    }

    #[test]
    #[cfg(feature = "gui")]
    fn test_image_card_constructs() {
        use super::plugin::ImageBlockData;
        use super::render::ImageCard;
        let card = ImageCard::new(
            ImageBlockData {
                url: "assets/demo_3.png".to_string(),
                alt: Some("Test Demo".to_string()),
                title: None,
                width: Some(900.0),
                height: None,
            },
            std::path::PathBuf::from("/home/Jake/dev/hadron/assets/demo_3.png"),
        );
        assert_eq!(card.data.width, Some(900.0));
    }
}



