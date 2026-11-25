---
name: ui-test-web
description: Test a web page the way a person uses it, by clicking and typing. Use when asked for UI tests, end-to-end tests, or to check that a flow like sign-up or checkout works.
metadata:
  where: code
---

# Test the web UI

The goal is proof that a person can do the thing: open the page, act, see the result.

## First: what does the project have?

Call `list_files` and look for `playwright.config.*`, `cypress.config.*`, or a `test:e2e` script in
`package.json`.

- **It has one**: follow "With the project's runner".
- **It has none**: follow "By looking". Do not install a test runner unless the person asks.

## With the project's runner

1. Read one existing test file whole. Copy its layout, its imports and its naming.
2. Write the flow as one test: go to the page, act, expect what the person would see.
   Find elements by their role or their text (`getByRole('button', { name: 'Save' })`), not by
   long CSS paths.
3. Expect something visible after each action: a text, a new address, a count.
4. Run it: `check_project` with `"what": "ui"`. It starts what the runner needs.
5. A failure names the step and what was expected. Decide which is wrong, the test or the page,
   before you change either.

## By looking

1. `start_app`, then `look_at_page` with the address. No console errors before you go on.
2. Write the flow down as steps, each with what should be true after it.
3. Do the steps with `do`, a few at a time:
   `look_at_page` with `"do": [{"type": "Email", "text": "a@b.co"}, {"click": "Sign up"}]`.
   A field is found by its CSS selector or its label; a button by its text.
4. Read the result: the text on the page, the console errors, the failed requests. Is the thing
   that should be true now on the page?
5. A step that "matches nothing" means the page has no such element. Look at the text of the page
   and use the words that are there.
6. Try one wrong input too: an empty field, a bad email. The page should say so, not break.

## Rules

- One flow at a time. Finish it before starting the next.
- Each `look_at_page` loads the page afresh: put all steps of one flow in one `do`.
- Do not weaken a test to make it pass.
- Say plainly which flows you tried and which you could not.

## The answer

Each flow on one line: what you did, what the page showed, pass or fail. Then what you fixed.
