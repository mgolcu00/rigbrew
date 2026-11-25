---
name: explain-codebase
description: Explain what a project is, how it is laid out and where to start reading. Use when asked to summarise, explain or give a tour of a repo or folder.
metadata:
  where: code
---

# Explain a codebase

The goal is a short, true picture of the project for someone who has never opened it.

## Steps

1. Call `list_files` on the top of the folder with depth 2. Note the language and the build files
   (`package.json`, `Cargo.toml`, `pyproject.toml`, `go.mod`, a `Makefile` and the like).
2. Read the README if there is one. Read the main build file to learn the name, the commands and
   the dependencies that matter.
3. Find where the program starts (`main`, `index`, `app`, the `bin` or `scripts` entries) and read
   the first 80 lines of that file.
4. Open at most three more files that look central: the ones the start file imports most, or the
   largest ones in the main source folder. Read the top of each; do not read everything.
5. If there is a test folder, list it. The test names say what the project promises to do.
6. Stop reading and answer. Ten tool calls are enough for almost any project.

## Checks before you answer

- Every command you name appears in a file you read.
- Every folder you describe is one you listed.
- You have not described a file you did not open.

## The answer

Write it in plain words, in this order:

- **What it is**, in one or two sentences.
- **Layout**: the main folders and what each one holds, one line each.
- **Run and test**: the exact commands you found.
- **Start reading here**: two or three file paths, with one reason each.
- **Open questions**: anything that looked unfinished or surprising, if you saw it.

Only say what you saw in the files. If you did not find something, say that you did not find it.
Do not change any file for this.
