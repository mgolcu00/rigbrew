---
name: preview-and-fix
description: Start the web app, look at it, and fix what is wrong until the page is right. Use after making or changing a page, or when the person says it looks wrong.
metadata:
  where: code
---

# Preview and fix

The goal is a page you have seen working, not one you believe works.

## Steps

1. **Start it.** Call `start_app` with no arguments. If it cannot tell how the project starts, give
   the command yourself and write `{port}` where the port goes. Read the last lines it shows: an
   error there comes first.
2. **Look.** Call `look_at_page` with the address `start_app` gave. Read all four parts:
   - the title and the text: is what the person asked for on the page?
   - console errors: each one names a file and a line;
   - failed requests: a 404 is a wrong path, a 500 is an error in the server;
   - the screenshot, when you can see it: layout, overlap, empty areas, unreadable text.
3. **Pick one problem**, the first error or the most visible fault. Say it in one sentence.
4. **Find it.** Open the file and line the error names with `read_file`. No error but a wrong look:
   `search` for the text or the class you see on the page.
5. **Fix it** with `edit_file`. One problem, one change.
6. **Look again** with `look_at_page`. Most dev servers load the change by themselves. If the page
   did not change, call `start_app` again.
7. Repeat from step 3 until there are no console errors, no failed requests, and the page shows
   what was asked for.
8. **Try it.** For a button or a form, call `look_at_page` with `do`, for example
   `[{"type": "#name", "text": "Ada"}, {"click": "Save"}]`, and read what the page says after.
9. **Check the small screen** once: `look_at_page` with `"size": "phone"`.

## Rules

- An empty page with no errors usually means the script did not run or the wrong address was
  opened. Look at the app's last lines and at the address.
- "Not rendered" means only the HTML was read. Say so; do not claim the page works.
- Do not hide an error to make the console clean.
- The same fault after three tries: stop and say what you saw and what you tried.

## The answer

What the page shows now, what you fixed (file and line), and what is still wrong or not checked.
