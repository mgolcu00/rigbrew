---
name: fix-failing-test
description: Find out why a test fails and fix the cause with the smallest change. Use when asked why a test fails, to fix a failing test, or to make the tests pass.
metadata:
  where: code
---

# Fix a failing test

The goal is one small change that makes the test pass for the right reason.

## Steps

1. Find how tests are run. Look at the build file (`package.json` scripts, `Cargo.toml`,
   `pyproject.toml`, a `Makefile`) or the README.
2. Run the tests with `run_command`. If you were told which test fails, run only that one.
3. Read the end of the output. Write down three things: the name of the failing test, the file and
   line of the failure, and what was expected against what came out.
4. Read the test. Then read the code it calls, starting at the line the output names.
5. Say the cause in one sentence before you change anything. If you cannot, read more; do not guess.
6. Decide which side is wrong. Most of the time the code is wrong and the test says what was meant.
   Change the test only when it clearly asks for the wrong thing, and say so.
7. Make the fix with `edit_file`. Change as few lines as you can. Do not tidy other code.
8. Run the same test command again. If it still fails, read the new output before changing more:
   a different message means you moved forward.
9. When it passes, run the whole test suite once to see that nothing else broke.

## Rules

- Never make a test pass by deleting it, skipping it or weakening what it checks.
- Never copy the expected value into the code just to match the test.
- If the person does not allow a command, do not ask for it again. Work from the files and say
  what you could not check.
- Stop after three attempts that did not work, and explain what you learned.

## The answer

Four short lines: what failed, why it failed, what you changed (file and line), and whether the
test and the rest of the suite pass now.
