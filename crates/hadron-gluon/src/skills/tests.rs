use super::*;
use std::path::Path;

use hadron_lattice::QuarkId;

use super::parse::parse_list_value;

fn skill_with_tools(tools: &[&str]) -> ResolvedSkill {
    ResolvedSkill {
        id: "s".into(),
        triggers: vec![],
        body: String::new(),
        description: None,
        tools: tools.iter().map(|t| t.to_string()).collect(),
    }
}

#[test]
fn no_tools_list_allows_everything() {
    let s = skill_with_tools(&[]);
    assert!(is_tool_allowed("read_file", &s));
    assert!(is_tool_allowed("run_bash", &s));
    assert!(is_tool_allowed("anything", &s));
}

#[test]
fn a_tools_list_allows_only_listed_tools() {
    let s = skill_with_tools(&["read_file", "grep_search"]);
    assert!(is_tool_allowed("read_file", &s));
    assert!(is_tool_allowed("grep_search", &s));
    assert!(!is_tool_allowed("run_bash", &s), "a tool not on the allow-list is denied");
    assert!(!is_tool_allowed("write_file", &s));
}

#[test]
fn tool_match_is_case_insensitive() {
    let s = skill_with_tools(&["Read_File"]);
    assert!(is_tool_allowed("read_file", &s));
    assert!(is_tool_allowed("READ_FILE", &s));
    assert!(!is_tool_allowed("read_files", &s), "no partial/substring match");
}

/// A skill whose `include_str!` points at the wrong path would compile only if the
/// file existed, but an EMPTY file compiles fine and injects nothing — a rule the
/// quark is silently never given.
#[test]
fn every_listed_skill_has_a_body() {
    assert!(!SKILLS.is_empty());
    for s in SKILLS {
        assert!(
            s.body.len() > 200,
            "skill `{}` has an empty or stub body ({} bytes) — it would inject nothing",
            s.id,
            s.body.len()
        );
        assert!(!s.triggers.is_empty(), "skill `{}` can never be selected", s.id);
    }
}

/// The always-on index names every skill with the description from its front-matter
/// — the "here are your procedures" list a quark gets every turn.
#[test]
fn the_index_lists_every_skill_with_its_description() {
    let idx = index(&builtins());
    for s in SKILLS {
        assert!(idx.contains(s.id), "index is missing skill `{}`", s.id);
    }
    assert!(
        description(SKILLS[0].body).is_some(),
        "a skill must carry a `description:` line for the index to quote"
    );
}

/// **The self-contained invariant.** No skill body may point at a companion file the
/// quark cannot open — every quark now gets only the index (a one-line summary) plus
/// the single active skill's body, so a body that leans on a sibling file or another
/// skill's prose has nothing to resolve against. "in this directory" and
/// "references/" are the Superpowers dangling-reference shapes; if one creeps back
/// with the next skill sync, this fails instead of shipping a dead end.
#[test]
fn no_skill_body_dangles_a_reference_the_quark_cannot_follow() {
    for s in SKILLS {
        let b = s.body.to_lowercase();
        assert!(!b.contains("in this directory"), "`{}` still says 'in this directory'", s.id);
        assert!(!b.contains("references/"), "`{}` still points at 'references/'", s.id);
    }
}

/// `render`'s `include_body` toggle: `true` inlines the procedure (what every quark
/// gets now, resident or one-shot alike); `false` only names the skill and points back
/// at the always-on index, without repeating the body.
#[test]
fn render_include_body_toggles_the_procedure_text() {
    let m = select("write a plan for X", &builtins()).unwrap();
    let me = QuarkId::new("opus");

    let full = render(&m, &me, &Handoff::default(), true);
    assert!(full.contains("author: <your quark id>"), "include_body=true must carry the body");

    let pointer = render(&m, &me, &Handoff::default(), false);
    assert!(pointer.contains("writing-plans"), "the skill is still named");
    assert!(
        !pointer.contains("author: <your quark id>"),
        "include_body=false must NOT repeat the body"
    );
    assert!(pointer.contains("skill index above"), "and must point back at the index, not a corpus");
}

#[test]
fn a_task_with_no_trigger_selects_nothing() {
    // The no-match path must be a true no-op: unchanged behaviour for every turn
    // that isn't plan work.
    assert!(select("fix the completion popup so it stops being clipped", &builtins()).is_none());
    assert!(select("", &builtins()).is_none());
}

#[test]
fn writing_a_plan_selects_the_writing_skill() {
    let m = select("Write a plan for the ACP auth work", &builtins()).expect("should match");
    assert_eq!(m.id, "writing-plans");
    assert_eq!(m.trigger, "write a plan");
}

#[test]
fn executing_a_plan_selects_the_executing_skill() {
    let m = select("Execute the plan at docs/plans/2026-07-14-acp-auth.md", &builtins()).expect("match");
    assert_eq!(m.id, "executing-plans");
}

#[test]
fn a_bare_plan_path_is_enough_to_select_executing() {
    let m = select("take docs/plans/2026-07-14-acp-auth.md and get it done", &builtins()).expect("match");
    assert_eq!(m.id, "executing-plans");
}

#[test]
fn reviewing_selects_the_review_skill() {
    let m = select("Review the plan agy wrote and tell me if it holds", &builtins()).expect("match");
    assert_eq!(m.id, "reviewing-work");
}

/// The turn has ONE phase. "Write a plan, then someone reviews it" is a writing
/// turn — the review is the next quark's job, and handing this quark both
/// procedures at once is how it ends up doing neither.
#[test]
fn the_earliest_mentioned_action_wins() {
    let m = select("Write a plan for X, then have someone code review it", &builtins()).expect("match");
    assert_eq!(m.id, "writing-plans", "the verb aimed at THIS quark comes first");

    let m = select("Review the plan, do not write a plan yourself", &builtins()).expect("match");
    assert_eq!(m.id, "reviewing-work");
}

/// A MISSED trigger is silent — the turn simply gets no procedure and nobody can
/// tell — so the phrasings a tired human actually types must all land.
#[test]
fn the_phrasings_a_human_actually_types_all_land() {
    let skills = builtins();
    for task in [
        "write up a plan for the auth work",
        "draft the plan first",
        "put together a plan for #34",
        "come up with a plan",
        "plan out the ACP auth work",
    ] {
        assert_eq!(
            select(task, &skills).map(|m| m.id),
            Some("writing-plans".to_string()),
            "missed: {task:?}"
        );
    }

    for task in ["implement this plan", "pick up the plan", "run the plan and commit"] {
        assert_eq!(
            select(task, &skills).map(|m| m.id),
            Some("executing-plans".to_string()),
            "missed: {task:?}"
        );
    }

    for task in ["review my work", "check the plan agy wrote", "verify the plan"] {
        assert_eq!(
            select(task, &skills).map(|m| m.id),
            Some("reviewing-work".to_string()),
            "missed: {task:?}"
        );
    }
}

#[test]
fn selection_is_case_insensitive() {
    assert_eq!(select("WRITE A PLAN for X", &builtins()).unwrap().id, "writing-plans");
}

#[test]
fn plan_ref_finds_a_path_through_human_punctuation() {
    assert_eq!(
        plan_ref("execute `docs/plans/2026-07-14-foo.md`, please."),
        Some("docs/plans/2026-07-14-foo.md".to_string())
    );
    assert_eq!(plan_ref("no plan here"), None);
    // A directory is not a plan, and a non-markdown file is not one either.
    assert_eq!(plan_ref("look in docs/plans/ for it"), None);
    assert_eq!(plan_ref("see docs/plans/foo.txt"), None);
}

#[test]
fn plan_author_reads_the_front_matter() {
    let md = "---\nauthor: opus\nstatus: draft\n---\n\n# A plan\n";
    assert_eq!(plan_author(md), Some(QuarkId::new("opus")));
}

/// Provenance is what the engine *checks*, so it must not be forgeable by prose:
/// a plan that merely discusses an author has no author.
#[test]
fn an_author_in_the_prose_is_not_provenance() {
    let md = "# A plan\n\nauthor: opus\n";
    assert_eq!(plan_author(md), None);

    let md = "---\nstatus: draft\n---\n\nauthor: agy\n";
    assert_eq!(plan_author(md), None);
}

#[test]
fn a_quark_handed_its_own_plan_is_told_to_hand_off() {
    let me = QuarkId::new("opus");
    let m = select("execute the plan", &builtins()).unwrap();
    let handoff = Handoff {
        peers: vec![QuarkId::new("agy")],
        plan_author: Some(QuarkId::new("opus")),
    };

    let out = render(&m, &me, &handoff, true);
    assert!(out.contains("you wrote this plan"), "must refuse self-verification:\n{out}");
    assert!(out.contains("`@agy`"), "must name the peer who can take it:\n{out}");
}

/// The inverse, and the one that must NOT fire: a plan written by someone else is
/// exactly the case we want executed, not bounced.
#[test]
fn a_plan_written_by_a_peer_is_not_a_conflict() {
    let me = QuarkId::new("opus");
    let m = select("execute the plan", &builtins()).unwrap();
    let handoff = Handoff {
        peers: vec![QuarkId::new("agy")],
        plan_author: Some(QuarkId::new("agy")),
    };

    let out = render(&m, &me, &handoff, true);
    assert!(!out.contains("you wrote this plan"));
    assert!(out.contains("independent pair of eyes"));
}

/// Live state, not a hypothetical: opus is disabled and acp-agy cannot boot, so a
/// lone quark is a real Saturday-night roster. It must not be told to hand work to
/// nobody.
#[test]
fn a_lone_quark_is_told_it_is_alone_rather_than_routed_into_the_void() {
    let me = QuarkId::new("opus");
    let m = select("write a plan for X", &builtins()).unwrap();
    let out = render(&m, &me, &Handoff::default(), true);

    assert!(out.contains("only quark"), "must say it is alone:\n{out}");
    assert!(out.contains("NOT been independently reviewed"));
}

/// Not an assertion — an affordance. `--ignored --nocapture` prints exactly what a
/// quark is handed for a given task, so a misfiring trigger is something you can
/// LOOK at instead of guess about.
#[test]
#[ignore = "prints the rendered skill; run with --ignored --nocapture"]
fn show_me_what_a_quark_actually_receives() {
    let task = "execute the plan at docs/plans/2026-07-14-acp-auth.md";
    let m = select(task, &builtins()).expect("should select a skill");
    let out = render(
        &m,
        &QuarkId::new("opus"),
        &Handoff {
            peers: vec![QuarkId::new("agy"), QuarkId::new("acp-claude")],
            plan_author: Some(QuarkId::new("opus")),
        },
        true,
    );
    println!("=== task: {task}\n{out}");
}

#[test]
fn the_selected_body_is_actually_injected() {
    let m = select("write a plan for X", &builtins()).unwrap();
    let out = render(&m, &QuarkId::new("opus"), &Handoff::default(), true);
    assert!(out.contains("Skill for this turn: writing-plans"));
    assert!(out.contains("author: <your quark id>"), "the real body must be present");
    assert!(out.contains("\"write a plan\""), "the trigger must be quoted back");
}

// --- Additive back-compat proof --------------------------------------------
//
// These pin the EXACT text `index`/`render` produce for a controlled, made-up
// skill set — not just "contains" assertions — so a change to the format
// strings (as opposed to a change to some real skill's prose, which is not
// this refactor's concern) fails these tests. Using a synthetic skill set
// rather than `builtins()` keeps the pin independent of the actual invariant
// files' wording, which is free to change for unrelated reasons.

#[test]
#[ignore = "temp: regenerate the real-builtins() index fixture"]
fn zzz_dump_real_index_fixture() {
    std::fs::write(
        concat!(env!("CARGO_MANIFEST_DIR"), "/testdata/builtins_index_snapshot.txt"),
        index(&builtins()),
    )
    .unwrap();
}

/// **The actual back-compat pin.** Unlike the synthetic-skill test below (which
/// pins the format independent of any real skill's prose), this pins `index`'s
/// output for the REAL `builtins()` — the whole reason this refactor exists is
/// that the engine's byte-stable prompt prefix (and its cache) must not move.
/// The fixture is checked in; regenerate it deliberately (via
/// `zzz_dump_real_index_fixture`, `--ignored`) when a skill's `name`/
/// `description`/`SKILLS` order actually changes — never to silence this test.
#[test]
fn index_over_builtins_matches_the_pinned_fixture() {
    let fixture = include_str!("../../testdata/builtins_index_snapshot.txt");
    assert_eq!(index(&builtins()), fixture, "index(&builtins()) output moved — was that intentional?");
}

#[test]
fn index_snapshot_pins_the_exact_line_format() {
    let skills = vec![
        ResolvedSkill {
            id: "alpha".to_string(),
            triggers: vec!["do alpha".to_string()],
            body: "alpha body".to_string(),
            description: Some("does alpha things".to_string()),
            tools: vec![],
        },
        ResolvedSkill {
            id: "beta".to_string(),
            triggers: vec!["do beta".to_string()],
            body: "beta body".to_string(),
            description: None,
            tools: vec![],
        },
    ];

    let expected = "\n# Your skills\n\nThese are your built-in procedures. The engine hands you a starting one below; invoke any of the others yourself as the work crosses into its kind (a bug while you execute → systematic-debugging; work done → requesting-code-review).\n\n- **alpha** — does alpha things\n- **beta** — \n";

    assert_eq!(index(&skills), expected);
}

#[test]
fn render_snapshot_pins_the_exact_wrapper_text() {
    let m = Match {
        id: "alpha".to_string(),
        body: "  alpha procedure body  \n".to_string(),
        trigger: "do alpha".to_string(),
    };
    let out = render(&m, &QuarkId::new("opus"), &Handoff::default(), true);

    let expected = "\n# Skill for this turn: alpha\n\nThe engine selected this procedure because your task says \"do alpha\". It is part of your working protocol for this turn — not a suggestion, and not optional. If it is the wrong procedure for what you were actually asked, say so in your report instead of half-following it.\n\n\nalpha procedure body\n\n## Who can take the next step\n\n**Nobody. You are the only quark that can take a turn right now** — every other seat is disabled, depleted, or absent. Do the work, and say plainly in your report that it has NOT been independently reviewed and by whom it still should be. Do not invent a reviewer: a line addressed to a disabled quark excites nobody and the work stops dead in the field.\n";

    assert_eq!(out, expected);
}

// --- load_skills / ResolvedSkill (Task 1) ------------------------------------

fn write_skill(dir: &Path, filename: &str, contents: &str) {
    std::fs::write(dir.join(filename), contents).unwrap();
}

#[test]
fn load_skills_with_no_dirs_equals_builtins() {
    let loaded = load_skills(None, None);
    let built = builtins();

    assert_eq!(loaded.len(), built.len());
    assert_eq!(
        loaded.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
        built.iter().map(|s| s.id.as_str()).collect::<Vec<_>>(),
    );
    assert_eq!(loaded, built, "no custom dirs must yield exactly the built-ins");
}

#[test]
fn repo_skill_overrides_builtin_by_name() {
    let repo = tempfile::tempdir().unwrap();
    write_skill(
        repo.path(),
        "writing-plans.md",
        "---\nname: writing-plans\ndescription: overridden\ntriggers: write a plan\n---\n\nTHE REPO VERSION OF THE BODY.\n",
    );

    let loaded = load_skills(None, Some(repo.path()));
    assert_eq!(loaded.len(), builtins().len(), "override replaces in place, does not add");

    let m = select("write a plan for X", &loaded).expect("trigger still matches");
    assert_eq!(m.id, "writing-plans");
    assert!(
        m.body.contains("THE REPO VERSION OF THE BODY."),
        "the repo body must win over the built-in:\n{}",
        m.body
    );
}

#[test]
fn global_then_repo_precedence() {
    let global = tempfile::tempdir().unwrap();
    let repo = tempfile::tempdir().unwrap();
    write_skill(
        global.path(),
        "custom.md",
        "---\nname: custom\ndescription: from global\ntriggers: do custom thing\n---\n\nGLOBAL BODY.\n",
    );
    write_skill(
        repo.path(),
        "custom.md",
        "---\nname: custom\ndescription: from repo\ntriggers: do custom thing\n---\n\nREPO BODY.\n",
    );

    let loaded = load_skills(Some(global.path()), Some(repo.path()));
    let custom = loaded.iter().find(|s| s.id == "custom").expect("custom skill present");
    assert!(custom.body.contains("REPO BODY."), "repo must win over global:\n{}", custom.body);
    assert!(!custom.body.contains("GLOBAL BODY."));
}

#[test]
fn custom_skill_is_selectable_by_its_triggers() {
    let repo = tempfile::tempdir().unwrap();
    write_skill(
        repo.path(),
        "foo-doer.md",
        "---\nname: foo-doer\ndescription: does foo\ntriggers: [foo, do the foo thing]\n---\n\nFOO PROCEDURE.\n",
    );

    let loaded = load_skills(None, Some(repo.path()));
    let m = select("please do foo now", &loaded).expect("custom trigger must match");
    assert_eq!(m.id, "foo-doer");
    assert!(m.body.contains("FOO PROCEDURE."));
}

/// A custom skill authored with an uppercase trigger (`triggers: [Foo]`) must
/// still be found by a lowercase task: `select` lowercases the task text before
/// matching, so an un-lowercased trigger would silently never match.
#[test]
fn custom_trigger_matches_case_insensitively() {
    let repo = tempfile::tempdir().unwrap();
    write_skill(
        repo.path(),
        "shouty.md",
        "---\nname: shouty\ndescription: shouts\ntriggers: [Frobnicate]\n---\n\nSHOUT PROCEDURE.\n",
    );

    let loaded = load_skills(None, Some(repo.path()));
    let m = select("please frobnicate the widget", &loaded).expect("uppercase trigger must still match");
    assert_eq!(m.id, "shouty");
    assert!(m.body.contains("SHOUT PROCEDURE."));
}

#[test]
fn front_matter_tools_parsed() {
    let repo = tempfile::tempdir().unwrap();
    write_skill(
        repo.path(),
        "restricted.md",
        "---\nname: restricted\ndescription: bounded tools\ntriggers: do restricted work\ntools: [read_file, grep_search]\n---\n\nBODY.\n",
    );

    let loaded = load_skills(None, Some(repo.path()));
    let s = loaded.iter().find(|s| s.id == "restricted").expect("present");
    assert_eq!(s.tools, vec!["read_file".to_string(), "grep_search".to_string()]);

    // The comma-separated form (no brackets) is accepted too.
    let repo2 = tempfile::tempdir().unwrap();
    write_skill(
        repo2.path(),
        "restricted2.md",
        "---\nname: restricted2\ndescription: bounded tools\ntriggers: do other work\ntools: read_file, write_file\n---\n\nBODY.\n",
    );
    let loaded2 = load_skills(None, Some(repo2.path()));
    let s2 = loaded2.iter().find(|s| s.id == "restricted2").expect("present");
    assert_eq!(s2.tools, vec!["read_file".to_string(), "write_file".to_string()]);
}

#[test]
fn index_lists_loaded_skills() {
    let repo = tempfile::tempdir().unwrap();
    write_skill(
        repo.path(),
        "foo-doer.md",
        "---\nname: foo-doer\ndescription: does foo things\ntriggers: foo\n---\n\nFOO.\n",
    );

    let loaded = load_skills(None, Some(repo.path()));
    let idx = index(&loaded);
    assert!(idx.contains("foo-doer"), "index must list the custom skill:\n{idx}");
    assert!(idx.contains("does foo things"));
    // The built-ins are still present too — this is a merge, not a replacement.
    assert!(idx.contains("writing-plans"));
}

#[test]
fn a_skill_file_with_no_name_is_skipped_not_guessed() {
    let repo = tempfile::tempdir().unwrap();
    write_skill(repo.path(), "nameless.md", "---\ndescription: no name here\n---\n\nBODY.\n");
    write_skill(repo.path(), "no-front-matter.md", "# just a heading\n\nno front matter at all.\n");

    let loaded = load_skills(None, Some(repo.path()));
    // Skipped, not guessed from the filename: neither the filename-derived id
    // nor any spurious entry shows up.
    assert!(!loaded.iter().any(|s| s.id == "nameless"));
    assert!(!loaded.iter().any(|s| s.id == "no-front-matter"));
    assert_eq!(loaded.len(), builtins().len(), "both bad files are skipped, not merged");
}

#[test]
fn a_missing_directory_yields_no_extra_skills() {
    let missing = Path::new("/nonexistent/hadron-skills-dir-that-does-not-exist");
    let loaded = load_skills(Some(missing), Some(missing));
    assert_eq!(loaded, builtins());
}

#[test]
fn parse_list_value_accepts_bracketed_and_bare_forms() {
    assert_eq!(parse_list_value("[a, b, c]"), vec!["a", "b", "c"]);
    assert_eq!(parse_list_value("a, b, c"), vec!["a", "b", "c"]);
    assert_eq!(parse_list_value("[\"a\", 'b']"), vec!["a", "b"]);
    assert_eq!(parse_list_value(""), Vec::<String>::new());
}

#[test]
fn skills_map_to_their_preferred_role() {
    assert_eq!(preferred_role("writing-plans"), Some("architect"));
    assert_eq!(preferred_role("brainstorming"), Some("architect"));
    assert_eq!(preferred_role("requesting-code-review"), Some("reviewer"));
    assert_eq!(preferred_role("reviewing-work"), Some("reviewer"));
    assert_eq!(preferred_role("executing-plans"), Some("executor"));
    assert_eq!(preferred_role("subagent-driven-development"), Some("executor"));
    assert_eq!(preferred_role("systematic-debugging"), None);
}
