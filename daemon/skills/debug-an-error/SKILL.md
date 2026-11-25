---
name: debug-an-error
description: Find the cause of an error message, a crash or wrong output, step by step. Use when the person pastes an error or a stack trace, or says something does not work.
---

# Debug an error

The goal is the cause, shown with evidence, and then the smallest fix. A guess that happens to
work is not the goal.

## Steps

1. **Read the error itself.** Quote the one line that says what went wrong. In a stack trace that
   is usually the last line; the first frame that belongs to the person's own code says where.
2. **Write down what you know**: what was run, what was expected, what happened instead, and
   whether it ever worked. If one of these is missing, ask for it or look for it. Ask one
   question at a time.
3. **Reproduce it** when you can run commands: run the smallest thing that shows the error. If you
   cannot run anything, work from the text and say that you could not check.
4. **Look at the place.** With file tools, read the file and line the error names, plus the
   function around it. Without them, ask the person to paste those lines.
5. **Name up to three possible causes**, most likely first. For each one say what you would see if
   it were true.
6. **Test the most likely one** with the cheapest check: read a value, search for where it is set,
   run one command, add one print. Change one thing at a time.
7. **When a check says no**, cross that cause off and take the next. Do not keep a theory the
   evidence does not fit.
8. **Fix the cause**, not the place where it showed. Then run the same thing that failed and see it
   pass.

## Common causes worth one look

- A name is misspelt, or a file is not where the code expects it.
- A value is empty, missing or of another type than the code assumes.
- A version differs: the code was written for another version of a library or tool.
- A step was skipped: not installed, not built, not saved, an old copy still running.
- Two things changed at once. Undo one.

## Rules

- Do not hide an error to make it go away (an empty catch, a skipped check).
- Do not change several things and hope. One change, one check.
- If the person said no to a command, do not ask again; find another way to look.
- After three causes ruled out, stop and say what you learned and what you would try next.

## The answer

- **Cause**: one or two sentences, with the evidence (the line, the value, the output).
- **Fix**: what to change, as exact text or a command.
- **Checked**: how you know it works, or what the person should run to see.
- **Not sure about**: anything you could not check.
