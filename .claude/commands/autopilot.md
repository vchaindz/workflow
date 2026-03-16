---
name: autopilot
description: Full development workflow orchestrator — chains research, implement, and review with user confirmations between phases. Usage - /autopilot <task description or #issue>
---

# Autopilot

You orchestrate the full development lifecycle by running three phases sequentially. Each phase corresponds to an existing command. You STOP between phases for user confirmation.

## Input

$ARGUMENTS

## Determine Mode

Parse the input to determine the workflow:

### 1. New task (description only)
Example: `/autopilot add template variable validation`

Full pipeline: Research → Implement → Review

### 2. Existing issue — implement (default)
Example: `/autopilot #42` or `/autopilot https://github.com/.../issues/42`

Skip research, go straight to implementation: Implement → Review

### 3. Existing issue — research first
Example: `/autopilot research #42` or `/autopilot research https://github.com/.../issues/42`

The issue exists but needs research before implementation (e.g., unclear scope, missing technical context, needs investigation). Runs: Research (enrich issue) → Implement → Review

**Detection**: if the input starts with the word `research` followed by an issue reference, use mode 3.

## Overview

```
Mode 1: [New task description]
  Phase 1: Research    →  /research (new issue)     →  Issue with context
           ↓ (user confirms)
  Phase 2: Implement   →  /implement                →  Working code + tests
           ↓ (user confirms)
  Phase 3: Review      →  /review                   →  Verified, ready to merge

Mode 2: [#XX or URL]
  Phase 2: Implement   →  /implement                →  Working code + tests
           ↓ (user confirms)
  Phase 3: Review      →  /review                   →  Verified, ready to merge

Mode 3: [research #XX or research URL]
  Phase 1: Research    →  /research (enrich issue)   →  Updated issue with context
           ↓ (user confirms)
  Phase 2: Implement   →  /implement                 →  Working code + tests
           ↓ (user confirms)
  Phase 3: Review      →  /review                    →  Verified, ready to merge
```

## Phase 1 — Research (`/research`) — Mode 1 and Mode 3 only

**You MUST invoke `/research` as a sub-command.** Do NOT attempt to run the research workflow manually — the command contains the full procedure including GitHub issue creation and codebase analysis.

Pass the original user input as args.

**At the end of this phase, present the issue to the user and STOP.**

Ask: "Issue created/updated. Ready to proceed to implementation, or do you want to adjust the scope?"

Possible user responses:
- **Proceed** → move to Phase 2
- **Adjust** → modify the issue based on feedback, then re-confirm
- **Stop here** → end the workflow (the issue is the deliverable)

## Phase 2 — Implement (`/implement`)

**You MUST invoke `/implement` as a sub-command.** Do NOT attempt to implement manually — the command contains the full procedure including branch setup, TDD workflow, and verification steps.

Pass the issue number from Phase 1 (e.g., `#42`) as args.

**At the end of this phase, present the implementation summary and STOP.**

Ask: "Implementation complete. Ready for review, or do you want to make changes first?"

Possible user responses:
- **Review** → move to Phase 3
- **Changes** → make adjustments, re-verify, then re-confirm
- **Stop here** → end the workflow (code is committed, skip review)

## Phase 3 — Review (`/review`)

**You MUST invoke `/review` as a sub-command.** Do NOT attempt to review manually — the command contains the full review pipeline.

No args needed — it reviews the current branch.

## Completion

After all three phases, produce a final summary:

### Workflow Summary

**Task**: [original input]
**Issue**: [link to GitHub issue]
**Branch**: [branch name]
**Commits**: [count] commits

### Phase Results
| Phase | Status | Key Output |
|-------|--------|------------|
| Research | ✓ | Issue #XX created |
| Implement | ✓ | N files changed, M tests added |
| Review | ✓ | X findings, Y fixed |

### Next Steps
- [ ] Create PR (suggest running `/pr`)
- [ ] Any deferred items

## Rules

- **ALWAYS invoke each phase as a sub-command** (`/research`, `/implement`, `/review`) — never execute phase workflows manually or from memory. The commands contain the complete procedures and must be invoked, not summarized.
- **NEVER commit directly to main or master** — always work on a feature branch
- **Always stop between phases** — never auto-proceed without user confirmation
- **Each phase is self-contained** — if the user stops early, the work from completed phases is preserved
- **Pass context forward** — Phase 2 uses the issue from Phase 1, Phase 3 reviews the code from Phase 2
- **Don't repeat work** — if Phase 1 already researched the codebase, Phase 2 should reference those findings rather than re-researching
- **Respect user's pace** — if the user wants to come back later for the next phase, that's fine; the issue and commits persist
