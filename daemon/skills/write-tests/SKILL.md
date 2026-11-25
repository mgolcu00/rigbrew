---
name: write-tests
description: Write tests for code that has none or too few. Use when asked to add tests, to cover a function or a file, or to test a bug before fixing it.
metadata:
  where: code
---

# Write tests

The goal is a few tests that would fail if the code were wrong, written the way this project
already writes its tests.

## Steps

1. Find the existing tests: `list_files`, then `search` for `test` in file names or for the test
   keyword of the language (`#[test]`, `def test_`, `it(`, `func Test`). Read one whole test file.
   Copy its layout, its imports and its naming.
2. Find the command that runs the tests (build file or README) and run it once with `run_command`,
   so you know the suite is green before you add anything.
3. Read the code to test. Write down what it promises: for this input, that output.
4. Pick the cases, in this order, and stop at about six:
   - the normal case, with a plain example;
   - the edges: empty, zero, one item, the largest size that makes sense;
   - the wrong input: what should be refused, and how;
   - one case for each `if` that changes the result.
5. Write the tests next to the existing ones with `edit_file`, or in a new file that follows the
   project's naming with `write_file`.
6. Run the tests. A new test that fails means one of two things: the test is wrong, or you found
   a bug. Read the code again and decide which. Do not change the code under test to make your
   test pass; report the bug instead.

## What a good test looks like

- Its name says what is promised: `empty_list_gives_zero`, not `test1`.
- It sets up, calls once, and checks the result. One reason to fail.
- It checks the value, not only that nothing crashed.
- It needs no network, no clock and no file outside a temporary folder.
- It does not repeat the code's own logic to work out the expected value. Write the expected value
  by hand.

## Rules

- Do not add a test library the project does not use.
- Do not weaken or delete tests that are there.
- If the code cannot be tested without changing it, say what small change would make it testable
  and stop.

## The answer

List the tests you added, one line each with what it proves. Say whether they pass, and name any
bug they found.
