---
name: fix-failing-test
description: Find out why a test fails and fix the cause with the smallest change. Use when asked why a test fails, to fix a failing test, or to make the tests pass.
---

# Fix a failing test

The goal is one small change that makes the test pass for the right reason.

## Steps

1. Find how tests are run. Look at the build file (`package.json` scripts, `Cargo.toml`,
   `pyproject.toml`, a `Makefile`) or the README.
2. Run the tests with `run_command`. If you were told which test fails, run only that one.
3. Read the end of the output. Find the name of the failing test, the file and line, and what was
   expected against what came out.
4. Read the test. Then read the code it calls.
5. Decide which side is wrong. Most of the time the code is wrong and the test says what was meant.
   Change the test only when it clearly asks for the wrong thing, and say so.
6. Make the fix with `edit_file`. Change as few lines as you can. Do not tidy other code.
7. Run the same test command again. If it still fails, read the new output before changing more.

## Rules

- Never make a test pass by deleting it, skipping it or weakening what it checks.
- If the person does not allow a command, do not ask for it again. Work from the files and say
  what you could not check.
- Stop after three attempts that did not work, and explain what you learned.

## The answer

Say what failed, why it failed, what you changed (file and line), and whether the test passes now.
