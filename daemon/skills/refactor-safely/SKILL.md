---
name: refactor-safely
description: Change how code is written without changing what it does. Use when asked to refactor, rename, tidy, split a long function or remove repetition.
metadata:
  where: code
---

# Refactor safely

The goal is code that reads better and behaves exactly as before. If behaviour has to change,
that is a different task: say so and stop.

## Before you touch anything

1. Say in one sentence what will be better afterwards. "Cleaner" is not an answer; "the price rule
   lives in one place instead of three" is.
2. Find the tests and run them with `run_command`. Note the result. If there are no tests for the
   code you will change, say so, and offer to write two or three first.
3. `search` for every use of what you will change: the function name, the type name, the file
   name in imports. Write the list down. These are the places that can break.

## Steps

Work in small moves. After each move the code must still build and the tests must still pass.

1. Make one move:
   - **Rename**: change the definition, then every use from your list. Search again for the old
     name; nothing may be left, in code, comments or docs.
   - **Extract**: copy the lines into a new function, call it from the old place, then delete the
     old lines. Keep the order of the arguments obvious.
   - **Remove repetition**: make the second copy call the first before you touch a third.
   - **Split a file**: move one thing at a time and fix the imports each time.
2. Run the tests. If they fail, undo the move with `edit_file` and take a smaller one.
3. Repeat. Stop when the sentence from the start is true.

## Rules

- No new behaviour, no bug fixes, no new dependencies on the way. Note what you find and report it.
- Do not change the public names other projects may use unless the person asked for that.
- Do not reformat lines you did not need to change; it hides the real change.
- Keep comments that explain why. Update the ones that describe what you moved.
- Three failed attempts at one move: stop and explain.

## The answer

- What is better now, in one sentence.
- The moves you made, one line each, with the files.
- The test result before and after.
- Anything you saw and left alone: possible bugs, more repetition, missing tests.
