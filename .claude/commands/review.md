---
name: review
description: Full pre-merge review pipeline — code review, security review, QA, fixes, and verification. Use when asked to review a branch, PR, or changes before merge. Covers code quality, security vulnerabilities, build/test verification, and automated fixes.
---

You are a team of specialists performing a comprehensive pre-merge review.

## Setup

- Identify the current branch and its base branch (determine dynamically — usually `main` or `master`)
- Get the diff of changed files: `git diff $(git merge-base HEAD <base-branch>)..HEAD --name-only`
- Focus the review ONLY on changed files and their immediate dependencies
- Categorize changed files: core modules (`src/core/`), TUI (`src/tui/`), CLI (`src/cli/`), templates (`templates/`), tests (`tests/`)

## Phase 1 — Reviews

Perform these three checks sequentially.

### 1. Code Review

Review changed files for:
- Code clarity, naming, and idiomatic Rust patterns
- Error handling: proper `Result<T>` propagation, no `unwrap()` in non-test code
- Ownership and borrowing correctness, lifetime issues
- `unsafe` usage (should be justified and minimal)
- `Clone` where `&` suffices, unnecessary allocations
- Duplication, dead code, overly complex logic
- Missing or misleading comments on public items
- Files approaching 500 lines (split recommendation per CLAUDE.md)
- Match conventions in the module (naming, structure, error handling via `DzError`)

### 2. Security Review

Review changed files for:
- Command injection vulnerabilities (especially in `executor.rs` — shell commands via `bash -c`)
- Path traversal in file discovery or template loading
- Secrets or credentials in code or templates
- Unsafe deserialization of YAML input
- Missing input validation on user-provided data
- TOCTOU races in file operations
- Information leakage in error messages
- Template expansion injection (`{{}}` variables)

### 3. QA Engineer

Run these checks on the project:

```bash
cargo build 2>&1
cargo clippy -- -D warnings 2>&1
cargo test 2>&1
cargo fmt --check 2>&1
```

Also:
- Check edge cases and error handling in changed code
- Validate no regressions by reviewing test coverage of changed lines
- If integration tests are relevant: `cargo test --test '*'`

### Reporting

After all three checks complete, output **detailed results** grouped by reviewer (Code Review → Security Review → QA).

For every finding include:
- **File and line number** where the issue is
- **Severity**: CRITICAL (must fix, blocks merge) / IMPORTANT (should fix) / MEDIUM (recommended) / LOW (nit, optional)
- **What exactly is wrong** — clear explanation with code context
- **Why it matters** — what can go wrong if left unfixed
- **Suggested fix** — concrete code or approach to resolve it

At the end, provide a summary count: X critical, Y important, Z medium, W low.

## Phase 2 — Fix Issues

**STOP and wait for explicit user confirmation before making any fixes.** Present Phase 1 findings and let the user decide which issues to fix, skip, or handle differently.

After confirmation, apply the approved fixes. Then re-run the Phase 1 checks on the modified files only.

**Limit: maximum 2 fix-and-recheck cycles.** If issues persist after 2 cycles, report remaining issues and stop — do not loop further.

## Phase 3 — Verification

Run full verification:

```bash
cargo build && cargo test && cargo clippy -- -D warnings && cargo fmt --check
```

Confirm everything is clean and ready.

## Final Report

After all phases complete, produce a concise report:

### Scope
- Branch name, base branch, list of changed files

### Findings and Fixes
- Table with columns: #, File:Line, Severity, Issue (brief), Status (fixed / skipped / deferred), Fix description (if applied)

### QA Results
- Build output (pass/fail)
- Test output (pass/fail, number of tests run)
- Clippy output (pass/fail)
- Format check (pass/fail)

### Final Status
- All checks passing: yes/no
- Remaining issues (if any): list with severity
- Ready to merge: yes/no
