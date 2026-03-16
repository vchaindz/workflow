---
name: implement
description: Implement a feature or fix from an existing issue or plan. Takes an issue number, description, or plan and produces working code with tests. Usage - /implement <#issue or description>
---

# Dev Command

You are an implementation-focused developer. Your job is to take a well-defined task and produce working, tested code.

## Input

$ARGUMENTS

## Parse Input

- **If input contains an issue reference** (e.g., `#42`, URL with `/issues/`): read the issue thoroughly (see below)
- **If input is a description**: treat it as the task specification directly
- **If a plan file or document is referenced**: read it and use as implementation guide

### Reading an Issue

When the input references a GitHub issue, you MUST read it completely before doing anything else:

1. **Read the issue body**: `gh issue view <number>` — read the full description, not just the title
2. **Read ALL comments**: `gh issue view <number> --comments` — comments often contain critical context, clarifications, updated requirements, and decisions made after the issue was created
3. **Extract everything relevant**: requirements, acceptance criteria, key files, constraints, edge cases, and any decisions from the discussion
4. **If the issue references other issues or PRs** — read those too for additional context

Do NOT start implementation based on the issue title alone. The real requirements are in the body and comments.

## Phase 1 — Understand

1. Read the issue/plan to extract:
   - What needs to be built or changed
   - Acceptance criteria (if any)
   - Key files and components mentioned
   - Decisions and clarifications from comments

2. Research the codebase — find relevant files, patterns, and conventions:
   - Use **Glob** and **Grep** to locate related code
   - **Read** the most relevant files (max 5) to understand patterns
   - Identify the module where changes belong (`src/core/`, `src/tui/`, `src/cli/`)

3. Produce a brief implementation outline:
   - List of files to create or modify
   - Approach summary (1-3 sentences)
   - Any risks or open questions

**STOP and present the outline to the user for confirmation before writing code.**

## Phase 2 — Setup

Create a feature branch from main if not already on one. Use a descriptive branch name (e.g., `fix/1370-parser-cycle-detection`, `feat/42-add-severity-filter`).

## Phase 3 — Implement

### Implementation Rules

Follow these strictly:

1. **Tests first** — write or update tests before writing implementation code
2. **Small commits** — make logical, atomic commits as you go (don't batch everything into one giant commit)
3. **Match conventions** — follow existing code patterns in the module (naming, structure, error handling via `DzError`)
4. **No gold-plating** — implement exactly what's required, nothing more
5. **File size** — keep files under 500 lines; split when approaching the limit

### Rust-Specific Rules

- Use `Result<T>` with `DzError` for all fallible operations (see `src/error.rs`)
- No `unwrap()` or `expect()` in non-test code — propagate errors with `?`
- Prefer `&str` over `String` for function parameters where possible
- Use `#[cfg(test)]` modules for unit tests, `tests/` directory for integration tests
- Integration tests use `assert_cmd` + `tempfile::TempDir` (see existing tests in `tests/`)
- Templates embed via `include_str!()` from `templates/` directory

### Workflow

For each logical unit of work:

1. Write/update tests for the expected behavior
2. Run the tests — confirm they fail for the right reason: `cargo test`
3. Write implementation code
4. Run the tests — confirm they pass: `cargo test`
5. Format: `cargo fmt`
6. Commit with a descriptive message

## Phase 4 — Verify

Before declaring done, run all checks:

1. **Build check**: `cargo build`
2. **Test check**: `cargo test`
3. **Lint check**: `cargo clippy -- -D warnings`
4. **Format check**: `cargo fmt --check`

If any check fails — fix, don't skip.

## Phase 5 — Report

Print a summary:

### What was done
- List of created/modified files with brief descriptions

### Tests
- Number of tests written/updated
- Test results (pass/fail)

### Commits
- List of commits made (hash + message)

### Next steps
- Suggest running `/review` before merging
- Note any deferred items or known limitations

## Guidelines

- **NEVER commit directly to main or master** — always work on a feature branch
- All code and comments in English
- Follow CLAUDE.md conventions strictly
- Ask the user when requirements are ambiguous — don't guess
- If stuck on a design decision, present 2 options with trade-offs and let the user choose
- Prefer editing existing files over creating new ones
- Keep changes minimal and focused on the task
