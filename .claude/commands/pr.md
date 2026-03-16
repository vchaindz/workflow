---
name: pr
description: Create a well-formatted GitHub PR from the current branch. Links to related issues, generates description from commits and diff. Use after implementation and review are complete. Usage - /pr [#issue or description]
---

# Create Pull Request

You create a well-formatted GitHub Pull Request from the current branch.

## Input

$ARGUMENTS

## Phase 1 — Gather Context

Run these checks in parallel:

1. **Current branch state:**
   ```bash
   git branch --show-current
   git status
   ```

2. **Base branch detection:**
   ```bash
   git merge-base --fork-point main HEAD 2>/dev/null || git merge-base --fork-point master HEAD 2>/dev/null
   ```

3. **Full diff and commit history from base:**
   ```bash
   BASE=$(git merge-base HEAD main 2>/dev/null || git merge-base HEAD master 2>/dev/null)
   git log --oneline "$BASE"..HEAD
   git diff "$BASE"..HEAD --stat
   ```

4. **Remote sync status:**
   ```bash
   git remote -v
   git rev-parse --abbrev-ref --symbolic-full-name @{u} 2>/dev/null || echo "No upstream set"
   ```

5. **Related issue (if provided):**
   ```bash
   gh issue view <number> --json title,body,labels,url
   ```

### Safety Checks

Before proceeding, verify:
- **Not on main/master** — refuse to create PR from main
- **No uncommitted changes** — warn if working tree is dirty
- **Branch has commits ahead of base** — refuse if nothing to merge

If any check fails, report the issue and STOP.

## Phase 2 — Analyze Changes

Read the full diff to understand what changed:

```bash
BASE=$(git merge-base HEAD main 2>/dev/null || git merge-base HEAD master 2>/dev/null)
git diff "$BASE"..HEAD
```

Categorize changes:
- **Core modules**: `src/core/` — parser, executor, models, discovery, etc.
- **TUI**: `src/tui/` — app state, actions, UI rendering
- **CLI**: `src/cli/` — args, dispatch
- **Templates**: `templates/` — embedded YAML workflows
- **Tests**: `tests/` — integration tests
- **Config/Build**: `Cargo.toml`, CI workflows

## Phase 3 — Draft PR

### Title

- Under 70 characters
- Format: `type: short description` (e.g., `feat: add severity filter to task list`)
- Types: `feat`, `fix`, `refactor`, `chore`, `docs`, `test`, `ci`
- If linked to an issue, reflect the issue's intent

### Body

Use this template:

```markdown
## Summary
<2-4 bullet points describing what this PR does and why>

## Changes
<Grouped list of notable changes by area>

## Testing
<What tests were added/modified, how to verify>

## Related
<Links to issues, other PRs, or context>

Closes #XX (if applicable)
```

Adapt the template:
- Skip empty sections
- Add breaking changes section if CLI interface or YAML format changed
- Add template changes section if embedded templates were modified

### Labels

Suggest labels based on changes (don't add labels that don't exist in the repo — check with `gh label list` first).

## Phase 4 — Confirm and Create

**STOP and present the draft (title + body) to the user for approval.**

After confirmation:

1. **Push branch** (if not already pushed):
   ```bash
   git push -u origin $(git branch --show-current)
   ```

2. **Create PR:**
   ```bash
   gh pr create --title "<title>" --body-file /tmp/pr-body.md --base main
   ```
   Write the body to a temp file first to preserve formatting.

3. **Link to issue** (if provided):
   The `Closes #XX` in the body auto-links. If additional linking is needed:
   ```bash
   gh pr edit <pr-number> --add-label "<label>"
   ```

4. **Report the result:**
   - PR URL
   - Summary of what was included
   - Reminder to monitor CI checks

## Rules

- **NEVER commit directly to main or master** — always work on a feature branch. If the user is on main/master, STOP and ask them to create a branch first.
- **NEVER force-push** — if the branch needs rebasing, ask the user
- **Always present the draft before creating** — the user must approve title and body
- Write PR descriptions in English
- Keep the description concise but complete — a reviewer should understand the PR without reading every line of code
- If the diff is very large (20+ files), suggest splitting into smaller PRs
