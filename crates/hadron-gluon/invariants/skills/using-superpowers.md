---
name: using-superpowers
description: Use when starting any conversation - establishes how to find and use skills, requiring skill invocation before ANY response including clarifying questions
---

<SUBAGENT-STOP>
If you were dispatched as a subagent to execute a specific task, ignore this skill.
</SUBAGENT-STOP>

<EXTREMELY-IMPORTANT>
If you think there is even a 1% chance a skill might apply to what you are doing, you ABSOLUTELY MUST invoke the skill.
IF A SKILL APPLIES TO YOUR TASK, YOU DO NOT HAVE A CHOICE. YOU MUST USE IT.
</EXTREMELY-IMPORTANT>

## Core Rule
Invoke relevant or requested skills BEFORE any response or action — including asking clarifying questions, exploring the codebase, or inspecting files.

- **Before Plan Mode:** If you have not already brainstormed, invoke `brainstorming` first.

## Skill Selection Order
Process skills govern approach and take precedence over domain/implementation skills:
- Feature Creation / Specs → `brainstorming` first.
- Bug Investigation / Failures → `systematic-debugging` first.

## Red Flag Rationalizations (STOP Instantly)
- "This is just a simple question" → Check for applicable skill first.
- "I need more context / explore first" → Skills define how to gather context.
- "The skill is overkill" → Follow procedure regardless of perceived complexity.
- "I know what this skill means without reading" → Read exact skill file every time.

## Priority Hierarchy
`User Directives (AGENTS.md / Chat instructions)` > `Skills` > `Default Behavior`.
Omit skill workflows ONLY when explicitly instructed by human partner.
