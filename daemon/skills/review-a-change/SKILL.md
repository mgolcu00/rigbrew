---
name: review-a-change
description: Review a code change for mistakes before it is kept. Use when asked to review a diff, a commit, a branch or the changes that are not committed yet.
metadata:
  where: code
---

# Review a change

The goal is a short list of real problems, most serious first. A review that finds nothing is a
fine result; do not invent problems.

## Steps

1. Get the change. Run `git status --short`, then `git diff` for work that is not committed, or
   `git diff main...HEAD` for a branch, or `git show <commit>` for one commit. If commands are not
   allowed, ask which files changed and read those.
2. Say in one sentence what the change is meant to do. If you cannot tell, that is the first
   finding.
3. For each changed file, read the changed lines and about 20 lines around them with `read_file`.
   A diff alone hides what the code next to it expects.
4. Go through the checks below for every change.
5. If the project has tests, find out whether the change is covered: `search` for the changed
   function's name in the test folder.

## Checks

- **Does it do what it says?** Follow one normal input through the new code by hand.
- **Edges**: empty input, zero, one item, a missing value, a very large value, text that is not
  English.
- **Errors**: what happens when the call it makes fails? Is the error hidden or passed on?
- **Callers**: `search` for other places that use what was changed. Do they still work?
- **Left behind**: debug prints, commented-out code, a TODO, a secret, a path from one machine.
- **Tests**: is the new behaviour tested? Was a test changed to make it pass?

## Rules

- Do not change any file. A review reports; it does not fix.
- Each finding names a file and a line and says what goes wrong, not only that it looks odd.
- Leave style alone unless it hides a bug.
- Say when you are not sure, and what would settle it.

## The answer

- One sentence: what the change does.
- **Must fix**: problems that break something. File, line, what happens, a suggested fix.
- **Should fix**: risks and missing tests.
- **Small things**: at most three.
- One line at the end: safe to keep, or not yet.
