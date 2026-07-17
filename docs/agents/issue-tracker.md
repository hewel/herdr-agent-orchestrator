# Issue tracker: GitHub

Issues and PRDs for this repo live as GitHub issues. Use the `gh` CLI for all operations.

## Conventions

- **Create an issue**: `gh issue create --title "..." --body "..."`
- **Read an issue**: `gh issue view <number> --comments`, including labels.
- **List issues**: use `gh issue list` with appropriate state and label filters.
- **Comment on an issue**: `gh issue comment <number> --body "..."`
- **Apply or remove labels**: use `gh issue edit`.
- **Close an issue**: `gh issue close <number> --comment "..."`

Infer the repository from `git remote -v`; `gh` does this automatically when run inside a clone.

## Pull requests as a triage surface

**PRs as a request surface: yes.**

External PRs run through the same labels and triage states as issues. Collaborators' in-progress PRs are excluded.

- **Read a PR**: `gh pr view <number> --comments` and `gh pr diff <number>`.
- **List external PRs**: use `gh pr list`, retaining authors with association `CONTRIBUTOR`, `FIRST_TIME_CONTRIBUTOR`, or `NONE`.
- **Comment, label, or close**: use the corresponding `gh pr` commands.

GitHub shares one number space across issues and PRs. Resolve an ambiguous number with `gh pr view <number>`, falling back to `gh issue view <number>`.

## When a skill says "publish to the issue tracker"

Create a GitHub issue.

## When a skill says "fetch the relevant ticket"

Run `gh issue view <number> --comments`.

## Wayfinding operations

The map is one GitHub issue with linked child issues.

- Label maps with `wayfinder:map`.
- Label children with `wayfinder:research`, `wayfinder:prototype`, `wayfinder:grilling`, or `wayfinder:task`.
- Prefer GitHub sub-issues and native issue dependencies.
- If those features are unavailable, use a task list in the map and a `Blocked by: #<number>` line in child issues.
- Claim work with `gh issue edit <number> --add-assignee @me`.
- Resolve work by commenting with the answer, closing the child issue, and adding a context pointer to the map.
