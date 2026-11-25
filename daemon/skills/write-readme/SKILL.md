---
name: write-readme
description: Write or refresh a README.md from what is really in the project. Use when asked for a README, for setup notes, or to document how to run the project.
---

# Write a README

The goal is a README that a newcomer can follow without asking anyone.

## Steps

1. Call `list_files` with depth 2. Read the build file and the file where the program starts.
2. If a README already exists, read it. Keep what is still true.
3. Collect the facts: the name, what it does, what it needs to be installed, the commands to
   install, run and test. Take commands from the build file, never from memory.
4. Write the file with `write_file` at `README.md` in the top of the folder.

## Shape

- A title and one sentence on what the project does.
- **Requirements**: what must be installed first, with versions only if the project states them.
- **Getting started**: the commands, each in a code block, in the order to run them.
- **Layout**: the main folders, one line each.
- **Tests**: the command to run them.
- **Licence**: only if the project has a licence file; name it.

## Rules

- Short sentences. No sales talk, no badges, no made-up features.
- Leave a section out when you found nothing for it.
- Keep it under about 80 lines unless the person asks for more.
- Do not paste secrets, tokens or personal paths into the file.

When it is written, say in two sentences what the README covers and what you could not find.
