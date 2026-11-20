# Rigbrew app

The desktop app: a Tauri 2 window around a SvelteKit single-page UI. The UI talks straight to
`rigbrewd` (the local service in `../daemon`) over HTTP on 127.0.0.1; the native side starts,
restarts and stops that service and hands the UI its port and token. The API is in
[`../docs/api.md`](../docs/api.md).

## Run

Once, from the repo root:

```sh
brew install pnpm
cargo build -p rigbrewd
cd app && pnpm install
```

### Native window

From the repo root:

```sh
export RIGBREW_DAEMON=$PWD/target/debug/rigbrewd
cd app && pnpm tauri dev
```

The service loads the signed manifest from `models.rigbrew.dev` and downloads the engine a model needs the first
time it is used. `RIGBREW_MANIFEST`, `RIGBREW_BASE_URL` and `RIGBREW_ENGINE_DIR` override this
([environment](../docs/api.md#environment)).

The app starts `rigbrewd` itself when no live one is found, and stops it on quit. With
`RIGBREW_DATA_DIR` set (the service inherits it) both sides use that folder, so a run against a
fresh, empty folder shows the first launch as a new install sees it. If a service
is already running (a live `daemon.json`), the app attaches to it and leaves it running.
Add `RIGBREW_BASE_URL=http://…` to download packages from a mirror; files are still checked against the signed manifest.

### Browser preview (no native window)

Terminal 1, from the repo root:

```sh
./target/debug/rigbrewd
```

Terminal 2:

```sh
cd app && pnpm dev        # http://localhost:1420
```

The dev server reads the service's `daemon.json` on every connect (`/__daemon`, see
`vite.config.ts`), so the preview follows a restarted service without any setup.
`VITE_RIGBREW_PORT` and `VITE_RIGBREW_TOKEN` in `.env.local` are only a fallback.
The port must be 1420: the service only accepts that origin in development.

Only in development, the address can replace what the server would say:

| Address | Shows |
|---|---|
| `/?welcome` | The first-launch flow again (nothing is deleted) |
| `/?config=soft` | The update notice and the sponsor sections |
| `/?config=hard` | The blocking update screen |
| `/?config=none&consent=x` | The consent step again ("The terms changed") |
| `/?welcome=scan`, `/?welcome=models` | The first-launch flow at that step, as a first launch sees it |
| `/?screen=models` (`device`, `profile`) | The app opened on that screen |
| `/?sheet=settings` (`privacy`), `/?palette`, `/?theme=dark` (`light`) | Settings, the palette, an appearance |
| `/?look` | The app without accepting the consent text (nothing is accepted; only basic usage data is kept). `VITE_RIGBREW_LOOK=1` does the same for the native window |

`curl 'localhost:1420/__view?screen=models&theme=dark'` sends the same to every window that is
already open, the native one included (`sheet=` closes the sheet, `palette=off` the palette).
More keys for `__view`, for screenshots and reviews:

| Key | Does |
|---|---|
| `look=1` | Enters the app from the consent step without accepting (kept for that window across reloads) |
| `go=/%3Fwelcome%3Dscan` (`models`) | Opens a first-launch step in that window, as `?welcome=scan` does, without accepting anything |
| `legal=privacy` (`terms`, `off`) | The Privacy Policy or the Terms of Use sheet |
| `reload=1` | Reloads every window (the native one does not always take hot updates of components) |
| `level=guided` (`standard`, `pro`), `text=small` (`default`, `large`, `larger`) | The experience level, the text size |
| `size=900x650` | Resizes the native window |
| `tour=0` to `3`, `tour=off` | A step of the first tour |
| `help=<article id>`, `help=off` | The Help sheet on one article (ids in `src/lib/help.ts`) |
| `tip=<id>`, `tip=off` | Shows a one-time tip again (`switch`, `install`, `nofit`, `picture`, `toolpicture`, `queued`, `offline`, `service`) |
| `suggest=standard` (`pro`, `off`) | The level suggestion, whatever the counts say |
| `fail=<package>`, `fail=off` | A failed download ("not enough disk space"), to see the error state |
| `chat=new`, `chat=<id>`, `model=<package>`, `pane=open` (`closed`) | A chat, Models opened on one model, the side pane |
| `checklist=reset`, `tips=reset`, `tourdone=reset` | Forget the checklist, the seen tips, the tour |

Settings > Privacy and data > Withdraw consent also brings the first-launch flow back, and
deletes the queued events.

## Check

```sh
pnpm check                          # svelte-check
(cd src-tauri && cargo check && cargo test --lib)   # the tests cover the sign-in callback
```

## Release build

```sh
pnpm release      # rigbrewd (release) inside the app; Rigbrew.app and a .dmg in src-tauri/target/release/bundle
```

`scripts/bundle-daemon.sh` builds `rigbrewd` and puts it where Tauri expects the sidecar
(`src-tauri/binaries/rigbrewd-<target>`); `src-tauri/tauri.release.conf.json` adds it to the
bundle, so `pnpm tauri dev` keeps working without it. The bundle is signed ad hoc: it runs on the
machine that built it. A download for other Macs needs a Developer ID signature and notarization,
which come with the beta. Test release builds with `DO_NOT_TRACK=1` so they are not counted as
installs.

## Interface

A tool, not a set of cards: 13px text, hairlines, one-row controls, everything on a 4px grid.

### How much it shows: Guided, Standard, Pro

One setting (Settings > How much to show, or the palette: "Switch to …") decides how much every
screen shows. A new install starts **Guided**. The levels and their flags are defined in one place,
`src/lib/experience.svelte.ts` (`ux.flags.plainNames`, `buildIds`, `speedWords`, `buildsTable`,
`keyHints`, `keysPane`, `tabDigits`, `ruler`, `queue`, `checklist`, `tips`, `hints`, …); components
read the flags and never ask for a level by name. Plain names and wording live in `src/lib/words.ts`.

| | Guided | Standard | Pro |
|---|---|---|---|
| Names | Function first: "Chat (Steep 4B)", "Pictures (Crema 6B)" | "Steep 4B · Balanced" with a Chat/Pictures tag | "Steep 4B q5" |
| Versions | Only the recommended one | Friendly names from the measurements (Smallest, Small, Balanced, Better, Best quality), "Other versions" under each model | Every build, raw ids, measured quality |
| Speed | "Fast on this Mac" when measured on this chip | "very fast", the number on hover | tokens per second |
| Models | One row per model, one action | Same, plus other versions | The full builds table |
| Bay | "Memory · Chat (Steep 4B) is loaded · plenty of room", a plain bar | The ruler with numbers, "12 to send" | Today's bay: GPU MEM, tok/s, idle, "12 queued" |
| Empty chat | "What would you like to do?", four starting rows, checklist, what is installed | Today's inventory, checklist, three prompts without routing | Today's |
| Keys | No key caps, no digits on tabs | Key caps on buttons and in the palette | Key caps, tab digits, the keys pane, slash hints |
| Help | Labelled Help button, "?" marks next to terms, all tips | "?" marks, tips for new things | "?" button only, no tips |

Switching says once, in the notice line, what changed. The app suggests the next level in one
quiet line above the bay ("Show more" / "Not now", which holds for a week), never switches by
itself, and only upwards: Guided to Standard after 10 messages and a picture (the service's own
totals) or two visits to Models; Standard to Pro after six palette or slash-command uses, or a
second version of a model installed. When the checklist is done it offers the next level too.

### Guidance

- **First tour** (`components/Tour.svelte`): after the first-launch flow, four marks over the real
  window (where to type, Chat or Picture, the memory line, Help and the profile). Esc or "Skip
  tour" ends it at any step; shown once; Help and the palette replay it.
- **Getting started** (`components/Checklist.svelte`, Guided and Standard): five steps that tick
  themselves (a question asked and a picture made, from `GET /v1/usage`; Models and the profile
  opened, the palette used, counted on this install). Collapsible; done, it folds into "You know
  the basics" and offers the next level.
- **Help** (`components/Help.svelte`, the words in `src/lib/help.ts`): the "?" in the title bar,
  Cmd/Ctrl+/ or F1. A search field, 14 short articles (what Rigbrew is, models and families, one
  model in memory, what fits, versions, pictures, the slow first answer, where data goes, disk
  space, updates, chats, the levels, shortcuts, a glossary). Small "?" marks next to terms
  (`components/Hint.svelte`) open the right article.
- **One-time tips** (`components/Tips.svelte`): a line above the bay, each shown once and
  remembered: the first model switch, the first install (size and folder), something that does
  not fit, the first picture, the first picture the chat model asked for, the first events
  waiting to be sent, going offline, the service restarting. Pro shows none.
- **Errors in plain words** (`src/lib/errors.ts`, `components/ErrorLine.svelte`): the service's
  messages (engine missing, no disk space, does not fit, could not start, connection lost, server
  not reachable…) become a sentence and one action (Try again, Free up space, What fits?). The raw
  message is under "Details": always in Pro, elsewhere when no sentence matched.
- **Text size** (Settings, the palette): Small, Default, Large, Larger. In the app the webview
  zooms (`setZoom`), so the layout reflows as at a narrower window; in the browser preview the
  page is zoomed with CSS.
- **Legal texts** (`components/LegalSheet.svelte`): "Privacy Policy" and "Terms of Use" on the
  consent step and in Privacy and data open the texts inside the app, rendered from
  `../docs/legal/privacy.md` and `terms.md`, imported at build time (`src/lib/legal.ts`, `?raw`;
  `vite.config.ts` allows that folder), so there is one source.
- **Accessibility**: answers are read out once finished through a polite live region; every icon
  button has a label and a tooltip; quiet text (`--faint`) is mixed to reach 4.5:1 on the panel in
  both themes; reduced motion is respected everywhere (`app.css`).

- **Title bar**: the mark, three places (Chat, Models, Your Mac; Device in Pro; Cmd/Ctrl+1 to 3),
  the way into the palette, new chat, the side pane switch, Settings, Help. There is no sidebar.
- **Palette** (Cmd/Ctrl+K, `components/Palette.svelte`): fuzzy search over actions (new chat,
  Picture mode, rename or delete this chat, unload, updates, send usage data), saved chats,
  places, models (chat with, install, show in Models), settings (level, text size, appearance)
  and help articles.
- **The bay** (`components/Bay.svelte`): one row along the bottom. The profile chip at the
  left; then the model memory budget as a ruled track (`components/Track.svelte`) with the
  model in memory seated in it as a chamfered module, to scale, labelled
  (`STEEP 4B q5 · 4.1 GB`). A click on the module offers Unload. States: nothing in memory,
  loading, switching (hatched module with a clock), generating (cursor on the module),
  reconnecting. Pointing at a build in Models draws where it would sit. Right of the track:
  tokens per second while an answer is written, running downloads, and the usage queue
  (`12 queued`, `sent 2m ago`).
- **Chat** is a transcript: left-aligned turns under one mono line each (`YOU`,
  `STEEP 4B q5 · 94 tok/s · 212 tok · 2.3 s`), pictures as one-line tool rows that open
  (`generate_image → Crema 6B · 1024×1024 · 8 steps · 11.4 s`), code blocks with Copy. The
  empty chat lists what is installed, what can be added, and three prompts that run on a
  click. The composer has mode chips and an inline `/` menu. A window of 1080px or more gets
  a side pane (this chat, chats, today, keys; Cmd/Ctrl+\; Guided shows only the chats).
  An open chat has a head line: its name (click to rename) and Delete.
- **Models** (Pro) is one table: families are groups, builds are rows (disk, memory against the
  budget as a small ruled track, quality, speed, state, actions). Any build that fits can be
  installed, not only the recommended one. Rows open in place; sort by column; filter by
  family, "fits" and "installed".
- **Device** is a spec sheet, the budget as the same ruled track, and every family seated in it.
- **Profile** (the chip at the bottom left): account and sign-in, a 53-week activity grid from
  `GET /v1/activity`, totals, streaks, most used models, recent activity, the usage queue.
- **Settings** (Cmd/Ctrl+,) is a sheet with one line per thing: appearance (system, light,
  dark), updates, storage, sponsors, account; then "Privacy and data" and
  "Export my data · Delete my account" as quiet lines.

## Brand

The look is locked in `../docs/brand` (rules in its README). In the app:

- **Colours**: `src/app.css` carries a copy of `docs/brand/tokens.css` (`--rb-*`), light and
  dark, following the system. Every other colour is one of those tokens or a mix of them.
  Green (`--fit`) is used only for "fits this machine".
- **Type**: Martian Mono (headings, model names, the wordmark), Departure Mono (labels and
  badges: 11px, uppercase; classes `.label` and `.badge`), Instrument Sans (text), Commit
  Mono (numbers, ids, code; class `.num`). All four are bundled, nothing is fetched at
  runtime: Martian Mono and Instrument Sans from `@fontsource-variable/*`, Departure Mono and
  Commit Mono from `static/fonts/` (copied from `docs/brand/fonts`, with their OFL texts).
  Faces are declared in `src/fonts.css` with `font-display: block` and preloaded, so text is
  drawn once and nothing shifts.
- **Mark**: the Bay mark, `src/lib/components/Mark.svelte` and `static/favicon.svg`.
- **App icon**: `src-tauri/app-icon.svg` is a copy of `docs/brand/app-icon.svg`. After
  changing it: `pnpm tauri icon src-tauri/app-icon.svg`, then delete the generated `android`,
  `ios`, `Square*`, `StoreLogo` and `64x64` files (only the five in `tauri.conf.json` are used).

## Chats are saved

`src/lib/chat.svelte.ts` saves each chat through the service after every completed turn
(`PUT /v1/chats/{id}`, debounced), reads the list on start (`GET /v1/chats`) and a chat's messages
when it is opened (`GET /v1/chats/{id}`). A chat is named after its first message; the name can be
changed in its head line or from the palette. Delete removes it from the list at once, offers Undo
for six seconds, then deletes its pictures (`DELETE /v1/images/files/{id}.png`) and the chat. Settings
deletes all of them. Pictures are kept as the id of the PNG the service saved
(`"image": {"id", "path", "bytes"}` in the image answer), never as base64; a saved picture needs the
token, so it is fetched once and shown through an object URL (the CSP allows `blob:` images).
With a service that answers 404 on `/v1/chats` the chats stay in memory and the side pane says so.
The profile's recent activity reads `GET /v1/activity/recent`.

## One chat, every installed model

Models are grouped in families (chat, image, video, voice, listening). Each installs
separately; the installed ones are used from the same chat.

- **Chat mode** sends the message to the chat model (`POST /v1/chat/completions`, streamed).
  When an image package is installed, the request carries one tool, `generate_image(prompt,
  size)`. If the model calls it, the app runs the image request, shows the picture as a card,
  adds the tool result to the history and lets the model finish its turn.
- **Picture mode** (the switch in the composer, Cmd/Ctrl+Shift+I, or `/image ` or `/picture ` at
  the start of a message) sends the text straight to the image package (`POST /v1/images/generations`). This
  is the dependable path: it does not rely on the chat model deciding to call a tool.
- A picture card says what made it and how ("Crema 6B · int8 · 1024 × 1024 · 8 steps ·
  11.4 s") and can copy the prompt, save the PNG (native save dialog in the app, a download in
  the browser preview) and regenerate with a new seed.
- The service keeps one engine in memory. Going from chat to image unloads the chat model and
  loads the image model; the card shows "Switching to Crema 6B…" with a running clock and a
  Stop button for that wait. The bay shows the engine in memory.
- With no image package installed, Image mode, a tool call, or a chat message that reads like
  a request for a picture puts an install offer in the conversation instead.
- Video (Pour), voice (Whistle) and listening (Sift) have no endpoint in the service yet. Models
  says so per family: "Download works; running it in Rigbrew comes later".
- Events sent through `POST /v1/events` (`src/lib/track.ts`; labels and numbers only, never what
  was typed): `app_opened`, `screen_viewed`, `chat_started`, `chat_stopped`, `mode_changed`,
  `image_requested`, `image_saved`, and for guidance `consent_shown`, `tour_started`, `tour_step`,
  `tour_finished`, `tour_skipped`, `checklist_item_done`, `checklist_completed`, `help_opened`,
  `help_article_opened`, `help_searched` (result count only), `tip_shown`, `tip_dismissed`,
  `level_changed`, `level_suggested`, `level_suggestion_accepted`, `level_suggestion_dismissed`,
  `text_size_changed`, `palette_opened`, `palette_action`, `slash_command`, `starter_clicked`,
  `install_clicked`, `remove_clicked`, `unload_clicked`, `chat_opened`, `chat_renamed`,
  `chat_deleted`, `chat_delete_undone`, `error_shown` (class only).

## Settings, account, usage

Settings is a sheet (Cmd/Ctrl+,). Privacy is not a screen of its own: Settings ends with one
quiet line, "Privacy and data", which opens the queue of events waiting to be sent, the
accepted consent, and Withdraw. Sign-in, usage and sync are on the Profile page; export and
delete are one line at the bottom of Settings.

- **Account.** Official builds add an account module behind `#account` (see "The account boundary"
  below); it is not part of this repository. Builds from this repository use `src/account-stub/`:
  there is no account, the profile shows local activity only, and no prompt or answer text is ever
  sent ([what leaves the machine](../docs/api.md#what-leaves-the-machine)).
- **Usage** from `GET /v1/usage`: messages, tokens in and out, images and time generating for
  today, 7 days and all time; disk per installed build with free space; how many events wait,
  when the last ones were sent, the last error, and "Send now" (`POST /v1/sync`). The chat's
  side pane shows today's line; it refreshes when an answer or a picture ends.
- **Updates** from `GET /v1/app/config` (through the service), asked on start, every 4 hours
  and by "Check now". `soft`: a notice in the corner that can be closed (remembered per
  version), with the notes and "Update", which opens `download_url`. `hard`: a screen that
  blocks everything until updated; a `hard` verdict marked `stale` (the server could not
  confirm it) is shown as a notice instead, so nobody offline is locked out. When
  `consent_version` differs from the accepted one, the consent step comes back. Events:
  `update_shown`, `update_accepted`, `update_dismissed`.
- **Sponsors** from `sponsors` in that config: a "Support Rigbrew" line in Settings (tiers,
  link) and one line under the Models header. Both open the system browser and send
  `sponsor_opened`. Without `sponsors` in the config neither is shown.

## The account boundary

Everything about accounts sits behind one import, `#account`. Only the stub is in this
repository; official builds put their own module at `src/account/` and `src-tauri/src/account.rs`
(both ignored by git) and build with `RIGBREW_ACCOUNT=1` and `--features account`:

| Path | What |
|---|---|
| `src/account/` | Official builds only: sign-in, the account part of the profile header (`AccountPanel`) and export and delete (`AccountDataLine`) |
| `src/account-stub/` | The same exports with nothing behind them: `getAccount()` reports no user, the components render nothing |
| `src-tauri/src/account.rs` | Official builds only: the native side of sign-in |

The interface (`index.ts` in both folders): `ACCOUNTS`, `getAccount()`, `onAccountChange()`,
`startAccount()`, `pauseAccount()`, `pushSession()`, `AccountPanel`, `AccountDataLine`. Nothing
outside the two folders imports anything else from them.

`vite.config.ts` points `#account` at the stub unless `RIGBREW_ACCOUNT=1` is set, for `pnpm dev`,
`pnpm build` and `pnpm tauri dev` alike. With the stub the profile shows local activity only,
Settings has no account lines, usage events carry no account id, and the bundle contains no
Firebase code (`rg -l 'initializeApp|firebase/' build` finds nothing; the word "Firebase" itself is
in the bundled Privacy Policy). `package.json` maps `#account` to the
stub as well, which is what `pnpm check` type-checks the app against; both folders are checked.

## Storage

- The Models header says what the installed builds take and how much is free; every Install
  button carries the download size.
- A running download can be cancelled (`DELETE /v1/jobs/{id}`); the partial file stays, so
  installing again resumes.
- On start and every 5 seconds the app reads `GET /v1/jobs` and attaches to running downloads,
  so progress survives a reload, an app restart and a service restart (the service resumes its
  downloads by itself).
- "Remove" on a build row calls `DELETE /v1/packages/{id}`
  (`?variant=`) after saying what it frees ("up to" when builds share files) and reports what
  was actually freed.
- When two builds of a package are installed, the row's details say which one is loaded (the
  best on disk) and that the other can be removed.
- The module in the bay opens to an Unload button (`DELETE /v1/engine`; Cmd/Ctrl+Shift+U).

## When the service stops answering

`src/lib/api.ts` notices a request that cannot reach the service (a heartbeat makes sure that
happens within 5 seconds), shows "Reconnecting to the local service" in the bay, and asks the native side for the connection again until it answers. The native
`daemon_info` command replaces a dead service and reads the new `daemon.json`, since port and
token change. After that the app re-reads consent, hardware and packages, re-attaches to
downloads and sends the session again. A request that was in flight fails with one sentence
that says to try again. The service is started with `RIGBREW_PARENT_PID`, so it exits by
itself when the app is killed.

## Layout

| Path | What |
|---|---|
| `src/app.css` | Brand tokens (light and dark), base styles, buttons, labels, badges |
| `src/fonts.css` | The four bundled typefaces |
| `src/lib/api.ts` | Client for `rigbrewd`; the only place that knows the port and token; reconnects. Also save file, copy text |
| `src/account/`, `src/account-stub/` | The account and its stand-in (see "The account boundary") |
| `src/lib/state.svelte.ts` | App state: consent, hardware, packages, downloads, usage, activity, app config (updates, sponsors), current screen, sheet, palette, appearance |
| `src/lib/chat.svelte.ts` | Conversations (saved through the service), modes, the streaming loop, tool calls, image jobs, saved pictures |
| `src/lib/format.ts` | Sizes, families and what each adds to chat, readable names for measured numbers |
| `src/lib/experience.svelte.ts` | The three levels and their flags, the text size |
| `src/lib/words.ts` | Plain words: function-first names, friendly version names, speed words, memory and fit sentences |
| `src/lib/guide.svelte.ts` | Help sheet state, the tour, tips, checklist counters, the level suggestion |
| `src/lib/help.ts` | The help articles (edit the words here) |
| `src/lib/errors.ts` | Service errors as plain sentences with one action |
| `src/lib/remember.ts`, `src/lib/track.ts` | localStorage with fallbacks; usage events |
| `src/lib/components/` | Title bar, bay, ruled track, palette, icon set, mark, install and remove controls, update notice, image row, model list (Guided/Standard), help, hint, tour, tips, checklist, level suggestion, error line |
| `src/lib/screens/` | Welcome (first launch), Chat, Models, Device, Profile, Settings (a sheet, with Privacy inside it) |
| `src/routes/+layout.svelte` | Font preloads; no browser context menu on chrome |
| `src/routes/+page.svelte` | Shell: title bar, the screen, the bay; the blocking update screen, global shortcuts |
| `src-tauri/src/lib.rs` | Starts, restarts and stops `rigbrewd`; commands `daemon_info`, `save_image`, `save_text` |
| `src-tauri/src/account.rs` | Sign-in through the system browser: `signin_listen`, `signin_wait`, `signin_cancel` |
| `src-tauri/tauri.conf.json` | Window: overlay title bar, vibrancy / Mica, CSP (the service, Firebase Auth, `api.rigbrew.dev`) |
| `vite.config.ts` | Dev server on 1420, `/__daemon` for the browser preview, `__APP_VERSION__` |

Imports inside `src/lib` use the `#lib/*` subpath alias (see `package.json`), which is what
SvelteKit 3 uses in place of `$lib`.

## Shortcuts

| Keys | Action |
|---|---|
| Enter / Shift+Enter | Send / new line |
| Esc | Stop the answer or the image |
| Cmd/Ctrl+N | New chat |
| Cmd/Ctrl+K | Palette: search chats, models, actions |
| Cmd/Ctrl+1 to 3 | Chat, Models, Device |
| Cmd/Ctrl+, | Settings |
| Cmd/Ctrl+/ or F1 | Help |
| Cmd/Ctrl+\ | The chat's side pane |
| Cmd/Ctrl+Shift+I | Switch the composer between Chat and Image |
| Cmd/Ctrl+Shift+U | Unload the model in memory |
| `/` | In the composer: `/image`, `/chat`, `/model`, `/new`, `/unload`, `/models` |

## Not here yet

- Chat, picture, voice and video models run. Voice reads answers aloud in English; a video clip
  takes a few minutes and blocks other models until it is done or stopped.
- On machines where a chat model and a picture model do not fit together, one picture asked for in
  Chat mode costs two engine swaps. Where they fit, both stay in memory and there is no swap.
- "Update" opens the download page; the in-app updater comes with the signed bundle.
- The bundle is not signed with a Developer ID yet, so it only runs on the Mac that built it.
- Windows is untested. On Windows the service is killed on quit rather than asked to stop,
  so its model engine may outlive it.
