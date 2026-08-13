---
name: writing-skills
description: Use when creating new skills, editing existing skills, or verifying skills work before deployment
---

# Writing Skills (TDD for Documentation)

Creating and editing skills follows Test-Driven Development (RED-GREEN-REFACTOR).

**Core Principle:** If you didn't watch an agent fail without the skill, you don't know if the skill teaches the right thing.

**REQUIRED BACKGROUND:** You MUST understand `test-driven-development` before using this skill.

## Core Authoring Principles

- **Concise is key:** Context window is shared resource. Target: getting-started workflows <150 words, frequently-loaded skills <200 words, other skills <500 words.
- **Degrees of Freedom:** Match specificity to fragility. High freedom for flexible workflows; exact low freedom (exact commands/guardrails) for fragile ops.
- **Consistent Terminology:** Pick one exact term per concept throughout.
- **No Time-Sensitive Data:** Document current standard; relegate legacy to old patterns.
- **Multi-Model Verification:** Test skills across target models.

## TDD Mapping for Skills

| TDD Phase               | Skill Creation Action                                  |
| ----------------------- | ------------------------------------------------------ |
| **Test Case**           | Pressure scenario / baseline task with subagent        |
| **Production Code**     | `SKILL.md` file                                        |
| **RED (Test Fails)**    | Agent violates rule / fails task without skill         |
| **GREEN (Test Passes)** | Agent complies / succeeds with skill present           |
| **REFACTOR**            | Close loopholes and refine without breaking compliance |

## Directory & File Structure

```
skills/
  <skill-name>/
    SKILL.md          # Main reference (required)
    supporting-file.* # Reusable tools or heavy reference (>100 lines) only
```

- Flat namespace under `.hadron/skills/`.
- Use separate files only for heavy reference (>100 lines) or executable scripts/templates. Keep principles and code patterns (<50 lines) inline.

## SKILL.md Structure & YAML Rules

```markdown
---
name: skill-name-with-hyphens
description: Use when [specific triggering conditions, symptoms, and contexts]
---
```

### Frontmatter Requirements:

- Max 1024 characters total.
- `name`: Lowercase letters, numbers, hyphens only.
- `description`:
    - Must start with `"Use when..."`.
    - Must be written in 3rd person.
    - **CRITICAL:** Describe ONLY triggering conditions/symptoms. **NEVER summarize the skill's workflow/process.** (Summarizing workflow causes agents to shortcut and skip reading full skill content).

### SDO & Keywords:

- Include error messages, symptoms, synonyms, and commands.
- Use active gerunds (`creating-skills`, `condition-based-waiting`).

### Cross-Referencing Other Skills:

- Format: `**REQUIRED SUB-SKILL:** skill-name`
- **FORBIDDEN:** Do NOT use `@` file imports (e.g. `@skills/foo/SKILL.md`) as they force-load full context prematurely.

## The Iron Law of Skills

```
NO SKILL WITHOUT A FAILING TEST FIRST
```

Applying to NEW skills AND EDITS to existing skills.
Written without testing? **Delete it. Start over.**

- No exceptions for "simple additions" or "doc updates".
- Delete means delete.

## Form Selection by Failure Type

| Baseline Failure Type          | Required Format                                        | Avoid                                     |
| ------------------------------ | ------------------------------------------------------ | ----------------------------------------- |
| Rule violation under pressure  | Prohibition + Rationalization Table + Red Flags        | Soft guidance ("prefer...")               |
| Wrong output shape / structure | Positive Recipe/Contract (specify exact parts & order) | Prohibition list ("don't narrate")        |
| Omitted required element       | Structural required slot in output template            | Prose reminders                           |
| Conditional behavior           | Explicit predicate (`if condition X, do Y`)            | Unconditional rule with exemption clauses |

_Rule:_ Avoid nuance clauses ("Don't X unless Y"). Express exceptions as explicit conditionals on observable predicates.

## Testing Skills (RED-GREEN-REFACTOR)

### 1. RED Phase (Baseline Test)

- Run pressure scenario with subagent WITHOUT skill.
- For discipline skills: combine 3+ pressures (time, sunk cost, authority, exhaustion, social proof, pragmatic shortcuts).
- Document exact baseline failures and verbatim rationalizations.

### 2. GREEN Phase (Minimal Skill)

- Write minimal `SKILL.md` addressing specific baseline failure rationalizations.
- Micro-test wording against a no-guidance control (5+ reps per variant).
- Run pressure scenario WITH skill and confirm compliance.

### 3. REFACTOR Phase (Close Loopholes)

- If agent finds new rationalization, add explicit counter in skill.
- Build Rationalization Table and Red Flags list.

```markdown
| Excuse               | Reality                                |
| -------------------- | -------------------------------------- |
| "Too simple to test" | Simple cases fail. Test takes seconds. |
```

## Anti-Patterns

- ❌ **Narrative Session Stories:** "In session X we found..." (Use reusable patterns instead).
- ❌ **Multi-Language Dilution:** Providing mediocre examples in 5 languages. (Provide 1 excellent, runnable example).
- ❌ **Code inside Flowcharts:** Put code in markdown blocks, not dot diagrams.
- ❌ **Generic Labels:** Using `step1`, `helper2` instead of semantic names.

## Mandatory Skill Creation Checklist

Create a tracking todo for each step:

### RED Phase

- [ ] Create pressure scenarios (3+ combined pressures for discipline rules).
- [ ] Run scenarios WITHOUT skill; record exact baseline failures and rationalizations.

### GREEN Phase

- [ ] Validate YAML frontmatter (`name` hyphenated, `description` starts with "Use when...", no workflow summary, <1024 chars).
- [ ] Ensure concise word count (<150 getting started, <200 frequent, <500 other).
- [ ] Format skill references as `**REQUIRED SUB-SKILL:** skill-name` (NO `@` links).
- [ ] Match guidance format to failure type.
- [ ] Run scenarios WITH skill; confirm compliance.

### REFACTOR Phase

- [ ] Add explicit counters for any new rationalizations.
- [ ] Populate Rationalization Table and Red Flags list.
- [ ] Re-test until bulletproof.
- [ ] Commit skill to git repository.
