---
name: explain-codebase
description: Explain what a project is, how it is laid out and where to start reading. Use when asked to summarise, explain or give a tour of a repo or folder.
---

# Explain a codebase

The goal is a short, true picture of the project for someone who has never opened it.

## Steps

1. Call `list_files` on the top of the folder with depth 2. Note the language and the build files
   (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, a `Makefile` and the like).
2. Read the README if there is one. Read the main build file to learn the name, the commands and
   the dependencies that matter.
3. Find where the program starts (`main`, `index`, `app`, the `bin` or `scripts` entries) and read
   the first part of that file.
4. Open at most three more files that look central. Read the top of each; do not read everything.
5. Stop reading and answer.

## The answer

Write it in plain words, in this order:

- What it is, in one or two sentences.
- How it is laid out: the main folders and what each one holds.
- How to run it and how to test it, with the exact commands you found.
- Where to start reading, as two or three file paths.
- Anything that looked unfinished or surprising, if you saw it.

Only say what you saw in the files. If you did not find something, say that you did not find it.
Do not change any file for this.
