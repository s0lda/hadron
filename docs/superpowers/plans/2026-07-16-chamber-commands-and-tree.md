# Chamber Commands and File Tree Improvements Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Implement pattern-based slash command coloring, sequential multi-command input execution, an Inspector tab tracking active plan checkboxes, and file tree disk improvements (showing all files, muting gitignored ones, and rendering directories first).

**Architecture:** 
*   **Command Styling**: Update `color_mentions` in `app.rs` to detect `/command` patterns at word boundaries and wrap them in custom HTML tags.
*   **Multi-Command Input**: Tokenize chat input on Enter to run matching UI commands sequentially and post any remaining text.
*   **Plan Tracker Tab**: Add a `Plan` tab to the Right Rail that scans the active turn's plan file via `plan_ref`, parses `- [ ]` vs `- [x]` checkboxes, and renders progress.
*   **File Tree Ignores & Sorting**: Union ignored and non-ignored workspace files from git, set recursive directory ignore flags, and render folders before files.

**Tech Stack:** Rust, GPUI, Git

## Global Constraints
*   Stage files by explicit path (e.g. `git add <path> <path>`). Never use `git add -A`, `git add .` or `git commit -a`.
*   Leave no scratch files behind (check `git ls-files --others --exclude-standard`).
*   Run full workspace gate `cargo test --workspace --features gui` to verify compilation and tests.

---

### Task 1: Slash Command Color Highlight (1.A)

**Files:**
*   Modify: `crates/hadron-chamber/src/app.rs`
*   Test: `cargo test -p hadron-chamber`

- [ ] **Step 1: Add a test asserting command coloring**
  Append the following test inside `mod tests` in `crates/hadron-chamber/src/app.rs`:
  ```rust
  #[test]
  fn test_color_commands() {
      let colored = color_mentions("Please run /plan and /grill-me today.", &[]);
      assert!(colored.contains("<span style=\"color: fuchsia-400\"><strong>/plan</strong></span>"));
      assert!(colored.contains("<span style=\"color: fuchsia-400\"><strong>/grill-me</strong></span>"));
      
      // Should ignore slashes inside code blocks
      let code = color_mentions("Code `/plan` inside.", &[]);
      assert!(!code.contains("color: fuchsia-400"));
  }
  ```
- [ ] **Step 2: Run test to verify it fails**
  Run: `cargo test -p hadron-chamber test_color_commands`
  Expected: FAIL (fuchsia color span missing)
- [ ] **Step 3: Implement pattern parsing in `color_mentions`**
  Modify `color_mentions` in `crates/hadron-chamber/src/app.rs` to parse `/` prefixes at word boundaries:
  ```rust
  fn color_mentions(body: &str, roster: &[crate::model::RosterRow]) -> String {
      let mut out = String::with_capacity(body.len() + 100);
      let mut chars = body.chars().peekable();
      let mut in_code_block = false;
      let mut in_inline_code = false;

      while let Some(c) = chars.next() {
          if c == '`' {
              let mut backtick_count = 1;
              while chars.peek() == Some(&'`') {
                  chars.next();
                  backtick_count += 1;
              }
              if backtick_count >= 3 {
                  in_code_block = !in_code_block;
              } else if !in_code_block {
                  in_inline_code = !in_inline_code;
              }
              for _ in 0..backtick_count {
                  out.push('`');
              }
              continue;
          }

          if c == '/' && !in_code_block && !in_inline_code && (out.is_empty() || out.ends_with(' ') || out.ends_with('\n')) {
              let mut cmd = String::new();
              while let Some(&nc) = chars.peek() {
                  if nc.is_alphanumeric() || nc == '-' || nc == '_' {
                      cmd.push(chars.next().unwrap());
                  } else {
                      break;
                  }
              }
              if !cmd.is_empty() {
                  out.push_str(&format!("<span style=\"color: fuchsia-400\"><strong>/{}</strong></span>", cmd));
              } else {
                  out.push('/');
              }
              continue;
          }
          // Existing '@' mention block follows...
  ```
- [ ] **Step 4: Run test to verify it passes**
  Run: `cargo test -p hadron-chamber test_color_commands`
  Expected: PASS
- [ ] **Step 5: Commit**
  Run: `git add crates/hadron-chamber/src/app.rs && git commit -m "feat(chamber): highlight slash commands in chat"`

---

### Task 2: Multi-Command Input Execution

**Files:**
*   Modify: `crates/hadron-chamber/src/app.rs`

- [ ] **Step 1: Update input submit handler for multicommand tokenization**
  Replace the prefix `/` command parsing in `on_input_submit` in `crates/hadron-chamber/src/app.rs` with:
  ```rust
          let text = input.read(cx).value().trim().to_string();
          if text.is_empty() {
              return;
          }

          let mut remaining_words = Vec::new();
          let mut executed_any_ui_cmd = false;
          let mut args_list = Vec::new();

          for word in text.split_whitespace() {
              if word.starts_with('/') && word.len() > 1 {
                  let cmd_name = &word[1..];
                  if ["toggle-roster", "toggle-inspector", "clear"].contains(&cmd_name) {
                      self.handle_chat_command(cmd_name, "", window, cx);
                      executed_any_ui_cmd = true;
                      continue;
                  } else if cmd_name == "team-brainstorm" {
                      args_list.push(word.to_string());
                      continue;
                  }
              }
              if !args_list.is_empty() {
                  args_list.push(word.to_string());
              } else {
                  remaining_words.push(word.to_string());
              }
          }

          if !args_list.is_empty() {
              let cmd = "team-brainstorm";
              let args = args_list[1..].join(" ");
              self.handle_chat_command(cmd, &args, window, cx);
              executed_any_ui_cmd = true;
          }

          let remaining_text = remaining_words.join(" ");
          if !remaining_text.is_empty() {
              let ev = Event::new(Actor::Human, None, Kind::Message { body: remaining_text });
              if let Err(e) = io::append_event(&self.path, &ev) {
                  eprintln!("chamber: failed to append event: {e}");
              } else {
                  let events = io::read_events(&self.path).unwrap_or_default();
                  self.reproject(&events);
                  self.chat_message_ixs = self
                      .view
                      .messages
                      .iter()
                      .enumerate()
                      .filter_map(|(ix, m)| (m.kind_label == "message").then_some(ix))
                      .collect();
                  for scroll in &self.chat_scrolls {
                      scroll.scroll_to_bottom();
                  }
                  self.chat_list_state.scroll_to_reveal_item(self.chat_message_ixs.len().saturating_sub(1));
              }
              executed_any_ui_cmd = true;
          }

          if executed_any_ui_cmd {
              input.update(cx, |state, cx| state.set_value("", window, cx));
              return;
          }
  ```
- [ ] **Step 2: Verify compiling and workspace tests**
  Run: `cargo test --workspace`
  Expected: PASS
- [ ] **Step 3: Commit**
  Run: `git add crates/hadron-chamber/src/app.rs && git commit -m "feat(chamber): support executing multiple UI commands in single input"`

---

### Task 3: Right Rail Plan Tracker Tab (2.A)

**Files:**
*   Modify: `crates/hadron-chamber/src/app.rs`

- [ ] **Step 1: Add Plan variant to `RightRailTab`**
  Modify `RightRailTab` enum in `crates/hadron-chamber/src/app.rs`:
  ```rust
  #[derive(Clone, Copy, PartialEq, Eq)]
  enum RightRailTab {
      Terminal,
      FileTree,
      Changes,
      Plan,
  }

  impl RightRailTab {
      const ALL: [RightRailTab; 4] = [
          RightRailTab::Terminal,
          RightRailTab::FileTree,
          RightRailTab::Changes,
          RightRailTab::Plan,
      ];

      fn index(self) -> usize {
          match self {
              RightRailTab::Terminal => 0,
              RightRailTab::FileTree => 1,
              RightRailTab::Changes => 2,
              RightRailTab::Plan => 3,
          }
      }

      fn label(self) -> &'static str {
          match self {
              RightRailTab::Terminal => "Terminal",
              RightRailTab::FileTree => "Files",
              RightRailTab::Changes => "Changes",
              RightRailTab::Plan => "Plan",
          }
      }
  ```
- [ ] **Step 2: Implement Plan Parser and rendering**
  Implement parsing of plan checklist items in `app.rs`:
  ```rust
  fn parse_plan_progress(content: &str) -> (usize, usize, Vec<(String, bool)>) {
      let mut total = 0;
      let mut completed = 0;
      let mut tasks = Vec::new();
      let mut current_task = String::new();

      for line in content.lines() {
          if line.starts_with("### Task") || line.starts_with("## Task") {
              current_task = line.trim_start_matches('#').trim().to_string();
          }
          if line.trim_start().starts_with("- [ ]") {
              total += 1;
              if !current_task.is_empty() {
                  tasks.push((format!("{}: {}", current_task, line.replace("- [ ]", "").trim()), false));
              }
          } else if line.trim_start().starts_with("- [x]") || line.trim_start().starts_with("- [X]") {
              total += 1;
              completed += 1;
              if !current_task.is_empty() {
                  tasks.push((format!("{}: {}", current_task, line.replace("- [x]", "").replace("- [X]", "").trim()), true));
              }
          }
      }
      (total, completed, tasks)
  }
  ```
- [ ] **Step 3: Render the Plan tab UI**
  Add rendering logic under `RightRailTab::Plan` match branch in `terminal_pane`:
  ```rust
              RightRailTab::Plan => {
                  let repo = crate::vcs::repo_root_of(&self.path).to_path_buf();
                  
                  // Attempt to find active plan mentioned in current task description
                  let active_plan_path = self.view.task.as_ref()
                      .and_then(|t| hadron_gluon::skills::plan_ref(t));

                  let plan_element = if let Some(rel_path) = active_plan_path
                      && let Some(content) = crate::sys::read_workspace_file(&repo, &rel_path)
                  {
                      let (total, completed, tasks) = parse_plan_progress(&content);
                      let pct = if total > 0 { (completed as f32 / total as f32 * 100.0) as usize } else { 0 };

                      let mut list = v_flex().gap_2().p_3().overflow_y_scroll();
                      list = list.child(div().font_weight(gpui::FontWeight::BOLD).child(format!("Active Plan: {}", rel_path)));
                      list = list.child(div().text_xs().text_color(theme::text_muted()).child(format!("Progress: {}/{} tasks completed ({}%)", completed, total, pct)));

                      for (task_desc, done) in tasks {
                          list = list.child(
                              h_flex().gap_2().items_center().child(
                                  Icon::new(if done { IconName::CircleCheck } else { IconName::Circle })
                                      .small()
                                      .text_color(if done { theme::glow_green() } else { theme::text_muted() })
                              ).child(div().text_sm().text_color(if done { theme::text_muted() } else { theme::text() }).child(task_desc))
                          );
                      }
                      list.into_any_element()
                  } else {
                      div().p_3().text_color(theme::text_muted()).child("No active implementation plan found in the current task.").into_any_element()
                  };

                  v_flex().flex_1().min_h_0().child(plan_element).into_any_element()
              }
  ```
- [ ] **Step 4: Verify compiling**
  Run: `cargo check -p hadron-chamber --features gui`
  Expected: Clean compilation.
- [ ] **Step 5: Commit**
  Run: `git add crates/hadron-chamber/src/app.rs && git commit -m "feat(chamber): add right rail plan checklist tracker"`

---

### Task 4: File Tree Gitignored Support

**Files:**
*   Modify: `crates/hadron-chamber/src/sys.rs`
*   Modify: `crates/hadron-chamber/src/app.rs`

- [ ] **Step 1: Update workspace file listing in `sys.rs` to return ignore status**
  Change `list_workspace_files` in `crates/hadron-chamber/src/sys.rs` to:
  ```rust
  pub fn list_workspace_files(repo_root: &Path) -> Vec<(String, bool)> {
      let mut files = vec![];
      // Tracked and untracked files (not ignored)
      if let Ok(output) = Command::new("git")
          .args(["ls-files", "--cached", "--others", "--exclude-standard", "--deduplicate"])
          .current_dir(repo_root)
          .output()
      {
          if output.status.success() {
              for line in String::from_utf8_lossy(&output.stdout).lines() {
                  if repo_root.join(line).exists() {
                      files.push((line.to_string(), false));
                  }
              }
          }
      }
      // Ignored files on disk
      if let Ok(output) = Command::new("git")
          .args(["ls-files", "--others", "--ignored", "--exclude-standard", "--deduplicate"])
          .current_dir(repo_root)
          .output()
      {
          if output.status.success() {
              for line in String::from_utf8_lossy(&output.stdout).lines() {
                  if !line.starts_with(".git/") && repo_root.join(line).exists() {
                      files.push((line.to_string(), true));
                  }
              }
          }
      }
      files.sort_by(|a, b| a.0.cmp(&b.0));
      files.dedup_by(|a, b| a.0 == b.0);
      files
  }
  ```
- [ ] **Step 2: Update tests in `sys.rs`**
  Modify tests in `crates/hadron-chamber/src/sys.rs` to assert correct tuples instead of simple strings.
- [ ] **Step 3: Update tree definitions in `app.rs`**
  Modify `file_tree_paths` field in `Chamber` in `app.rs` to use `Vec<(String, bool)>` and map to simple strings for completion files:
  ```rust
      // In Chamber definition:
      file_tree_paths: Vec<(String, bool)>,
  ```
  And in initialization:
  ```rust
          let files = crate::sys::list_workspace_files(&repo_root);
          let paths: Vec<String> = files.iter().map(|(p, _)| p.clone()).collect();
          let completion_files = std::rc::Rc::new(std::cell::RefCell::new(paths));
  ```
- [ ] **Step 4: Update recursive `FileTreeNode` to resolve ignores**
  Modify `FileTreeNode` in `crates/hadron-chamber/src/app.rs`:
  ```rust
                      struct FileTreeNode {
                          children: std::collections::BTreeMap<String, FileTreeNode>,
                          is_file: bool,
                          is_ignored: bool,
                          full_path: String,
                      }
                      impl FileTreeNode {
                          fn insert(&mut self, path: &str, full_path: &str, is_ignored: bool) {
                              let mut current = self;
                              let parts: Vec<&str> = path.split('/').collect();
                              for (i, part) in parts.iter().enumerate() {
                                  let is_file = i == parts.len() - 1;
                                  current =
                                      current.children.entry(part.to_string()).or_insert_with(|| {
                                          FileTreeNode {
                                              children: std::collections::BTreeMap::new(),
                                              is_file,
                                              is_ignored,
                                              full_path: if is_file {
                                                  full_path.to_string()
                                              } else {
                                                  String::new()
                                              },
                                          }
                                      });
                                  if is_ignored {
                                      current.is_ignored = true;
                                  }
                              }
                          }
                          
                          fn resolve_ignores(&mut self) -> bool {
                              if self.is_file {
                                  self.is_ignored
                              } else {
                                  if self.children.is_empty() {
                                      self.is_ignored
                                  } else {
                                      let mut all_ignored = true;
                                      for child in self.children.values_mut() {
                                          if !child.resolve_ignores() {
                                              all_ignored = false;
                                          }
                                      }
                                      self.is_ignored = all_ignored;
                                      all_ignored
                                  }
                              }
                          }
                      }
  ```
- [ ] **Step 5: Verify compiling**
  Run: `cargo test -p hadron-chamber`
  Expected: PASS
- [ ] **Step 6: Commit**
  Run: `git add crates/hadron-chamber/src/sys.rs crates/hadron-chamber/src/app.rs && git commit -m "feat(chamber): fetch and flag gitignored files in file tree"`

---

### Task 5: File Tree Rendering Polish (Folders First & Muting)

**Files:**
*   Modify: `crates/hadron-chamber/src/app.rs`

- [ ] **Step 1: Sort tree folders first and mute text of ignored nodes**
  Update `render_node` inside `RightRailTab::FileTree` rendering block in `crates/hadron-chamber/src/app.rs`:
  ```rust
                      fn render_node(
                          name: &str,
                          node: &FileTreeNode,
                          depth: usize,
                          cx: &mut Context<Chamber>,
                          repo_root: &std::path::PathBuf,
                          current_path: String,
                          expanded_set: &std::collections::HashSet<String>,
                      ) -> gpui::AnyElement {
                          let mut list = v_flex().w_full();
                          if name.is_empty() {
                              // Sort folders before files alphabetically
                              let mut children: Vec<(&String, &FileTreeNode)> = node.children.iter().collect();
                              children.sort_by(|(a_name, a_node), (b_name, b_node)| {
                                  match (a_node.is_file, b_node.is_file) {
                                      (false, true) => std::cmp::Ordering::Less,
                                      (true, false) => std::cmp::Ordering::Greater,
                                      _ => a_name.cmp(b_name),
                                  }
                              });

                              for (child_name, child_node) in children {
                                  let child_path = child_name.clone();
                                  list = list.child(render_node(
                                      child_name,
                                      child_node,
                                      depth,
                                      cx,
                                      repo_root,
                                      child_path,
                                      expanded_set,
                                  ));
                              }
                              return list.into_any_element();
                          }

                          let is_expanded = expanded_set.contains(&current_path);
                          let text_color = if node.is_ignored {
                              theme::text_muted()
                          } else {
                              theme::text()
                          };

                          let row = h_flex()
                              .id(SharedString::from(format!("tree-row-{}", node.full_path)))
                              .px_2()
                              .py_1()
                              .ml(gpui::px(depth as f32 * 12.0))
                              .hover(|s| s.bg(theme::bg_surface_raised()))
                              .cursor_pointer()
                              .text_color(text_color)
                              .font_family("Cascadia Code")
                              .text_size(gpui::px(13.56))
                              .gap_2()
                              .child(if node.is_file {
                                  Icon::new(IconName::File)
                                      .small()
                                      .text_color(theme::text_muted())
                                      .into_any_element()
                              } else {
                                  Icon::new(if is_expanded {
                                      IconName::FolderOpen
                                  } else {
                                      IconName::Folder
                                  })
                                  .small()
                                  .text_color(theme::text_muted())
                                  .into_any_element()
                              })
                              .child(div().child(name.to_string()));
  ```
- [ ] **Step 2: Run full gate verify**
  Run: `cargo test --workspace --features gui`
  Expected: PASS
- [ ] **Step 3: Commit**
  Run: `git add crates/hadron-chamber/src/app.rs && git commit -m "feat(chamber): render file tree folders first and mute gitignored files"`

---

### Task 6: Bug Fixes (File Preview Caching & Live Autocomplete Rescan)

**Goal:** Fix the file tree preview cache locking on the first opened file (by clearing the key `usize::MAX` when previews change) and fix newly created files not showing up for `@` mentions unless the File Tree pane is on screen (by removing the tab check from the 400ms tick file rescan).

**Files:**
*   Modify: `crates/hadron-chamber/src/app.rs`

- [x] **Step 1: Fix file preview cache**
  In `crates/hadron-chamber/src/app.rs`, clear the cache entry for `usize::MAX` in the `parsed_markdown` cache whenever a file is opened or closed:
  - Inside `ContextMenuAction::OpenFile(path)`:
    ```rust
                    self.parsed_markdown.borrow_mut().remove(&usize::MAX);
                    self.file_tree_open = Some((path, content));
    ```
  - Inside the close-file button click handler:
    ```rust
                                        this.parsed_markdown.borrow_mut().remove(&usize::MAX);
                                        this.file_tree_open = None;
    ```
  - Inside the file tree node double-click handler:
    ```rust
                                            this.parsed_markdown.borrow_mut().remove(&usize::MAX);
                                            this.file_tree_open = Some((file_name.clone(), content));
    ```
- [x] **Step 2: Rescan files unconditionally for live autocomplete**
  In `crates/hadron-chamber/src/app.rs`, run the file rescan and update `completion_files` on every 400ms tick, removing the `self.right_rail_tab == RightRailTab::FileTree` gate:
  ```rust
            // Rescan files unconditionally so autocomplete mentions are always live,
            // regardless of which right rail tab is active.
            let root = crate::vcs::repo_root_of(&self.path);
            let files = crate::sys::list_workspace_files(root);
            if files != self.file_tree_paths {
                *self.completion_files.borrow_mut() = files.clone();
                self.file_tree_paths = files;
                changed = true;
            }
  ```
- [ ] **Step 3: Run full gate verify**
  Run: `cargo test --workspace --features gui`
  Expected: PASS
- [ ] **Step 4: Commit**
  Run: `git add crates/hadron-chamber/src/app.rs && git commit -m "fix(chamber): resolve file tree preview caching and live autocomplete rescan"`

---

### Task 7: Keyboard Navigation and Tab Controls (Hadron Keyboard-First Goal)

**Goal:** Make Hadron highly keyboard-navigable by adding tab switching, stats sub-tab controls, Alt menu opening, Tab key focus toggling, Shift+Enter chat autoscroll, and Roster quark navigation.

**Files:**
*   Modify: `crates/hadron-chamber/src/app.rs`
*   Modify: `crates/hadron-chamber/src/model.rs`

- [ ] **Step 1: Define GPUI Actions for keyboard controls**
  In `crates/hadron-chamber/src/app.rs`, define the new actions at the top of the file:
  ```rust
  actions!(chamber, [
      CycleMode,
      NextChatTab,
      PrevChatTab,
      NextInspectorTab,
      PrevInspectorTab,
      NextStatsSubTab,
      PrevStatsSubTab,
      OpenMenu,
      ToggleFocus,
      NextQuark,
      PrevQuark,
      ToggleSelectedQuark,
  ]);
  ```
- [ ] **Step 2: Map Key Bindings**
  In `Chamber::init` (where `cx.bind_keys` is called), register the key bindings for different platforms:
  ```rust
  // macOS Cmd-bindings
  #[cfg(target_os = "macos")]
  cx.bind_keys([
      KeyBinding::new("cmd-right", NextChatTab, Some(KEY_CONTEXT)),
      KeyBinding::new("cmd-left", PrevChatTab, Some(KEY_CONTEXT)),
      KeyBinding::new("cmd-shift-right", NextInspectorTab, Some(KEY_CONTEXT)),
      KeyBinding::new("cmd-shift-left", PrevInspectorTab, Some(KEY_CONTEXT)),
      KeyBinding::new("cmd-down", NextStatsSubTab, Some(KEY_CONTEXT)),
      KeyBinding::new("cmd-up", PrevStatsSubTab, Some(KEY_CONTEXT)),
  ]);

  // Non-macOS Ctrl-bindings
  #[cfg(not(target_os = "macos"))]
  cx.bind_keys([
      KeyBinding::new("ctrl-right", NextChatTab, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-left", PrevChatTab, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-shift-right", NextInspectorTab, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-shift-left", PrevInspectorTab, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-down", NextStatsSubTab, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-up", PrevStatsSubTab, Some(KEY_CONTEXT)),
  ]);

  // Global bindings (Alt, Tab, Quark nav)
  cx.bind_keys([
      KeyBinding::new("alt", OpenMenu, Some(KEY_CONTEXT)),
      KeyBinding::new("tab", ToggleFocus, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-alt-down", NextQuark, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-alt-up", PrevQuark, Some(KEY_CONTEXT)),
      KeyBinding::new("ctrl-alt-enter", ToggleSelectedQuark, Some(KEY_CONTEXT)),
  ]);
  ```
- [ ] **Step 3: Implement Actions in Chamber view**
  In `Chamber::render`'s root element layout, attach the `.on_action` listeners:
  - **`NextChatTab` / `PrevChatTab`**: Cycle the `self.chat_tab` forward or backward using `ChatTab::ALL`.
  - **`NextInspectorTab` / `PrevInspectorTab`**: Cycle the `self.right_rail_tab` forward or backward using `RightRailTab::ALL`.
  - **`NextStatsSubTab` / `PrevStatsSubTab`**: If `self.chat_tab == ChatTab::Stats`, cycle `self.stats_window` forward or backward using `StatsWindow::ALL`.
  - **`OpenMenu`**: Set `self.app_menu_open = !self.app_menu_open;` and call `cx.notify()`.
  - **`ToggleFocus`**: Toggle window focus between `self.input` and the terminal focus handle (if the Terminal tab is visible).
  - **`NextQuark` / `PrevQuark`**: Update `self.selected_quark_ix: Option<usize>` to select quarks in `self.team.quarks` (cycle index).
  - **`ToggleSelectedQuark`**: Open the context menu for the selected quark.
- [ ] **Step 4: Implement Alt-triggered Menu Overlay**
  - Add `app_menu_open: bool` to the `Chamber` struct state, initialized to `false`.
  - Update `menu_button` or the main view render function to render the popup context menu absolutely if `self.app_menu_open` is true. Ensure a dismiss/click-outside handler resets the boolean to `false`.
- [ ] **Step 5: Improve Chat Auto Scroll on Shift+Enter**
  - Modify `InputState` key down event capturing. When `Shift+Enter` is intercepted:
    - Allow standard newline insertion.
    - Queue `window.on_next_frame` callback to scroll the active chat tab scroll handle to the bottom after the new input height is computed.
- [ ] **Step 6: Roster keyboard selection visual cue**
  - In `roster_row` rendering, check if the current quark's index in the list matches `self.selected_quark_ix`. If so, draw a subtle fuchsia/glow outline or background highlight to show keyboard focus.
- [ ] **Step 7: Run full gate verify**
  Run: `cargo test --workspace --features gui`
  Expected: PASS
- [ ] **Step 8: Commit**
  Run: `git add crates/hadron-chamber/src/app.rs crates/hadron-chamber/src/model.rs && git commit -m "feat(chamber): add keyboard tab controls and navigation"`
