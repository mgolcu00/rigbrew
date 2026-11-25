# The local API

The app never runs a model itself. It talks to `rigbrewd`, a small service on the same machine, and `rigbrewd`
runs the engines. Anything that can speak HTTP can use the same API: the chat endpoint is OpenAI-compatible, so
existing clients work against your own machine.

## Connecting

`rigbrewd` listens on `127.0.0.1` only, on a random port, and writes the port and a random token to
`daemon.json` in the data folder (`~/Library/Application Support/Rigbrew` on macOS) with mode `0600`.
Every request needs `Authorization: Bearer <token>` (all but `/v1/artifacts/frame`, see below).

```sh
D="$HOME/Library/Application Support/Rigbrew/daemon.json"
curl -s -H "Authorization: Bearer $(jq -r .token "$D")" "http://127.0.0.1:$(jq -r .port "$D")/v1/system"
```

Browsers are refused unless the request comes from the app's own origin.

## Endpoints

| Method and path | What it does |
|---|---|
| `GET /v1/system` | Hardware profile: CPU, memory, GPU, disk, hardware class and memory budgets. |
| `GET /v1/packages` | Every package with its versions, the version recommended for this machine and why, and what is installed. |
| `POST /v1/packages/{id}/pull` | Start a download, optionally `{"variant": "q5"}`. Returns a job id. The engine the model needs comes with it. |
| `DELETE /v1/packages/{id}` | Remove installed files (`?variant=q5` for one version). Files another installed version still uses are kept. |
| `POST /v1/community/lookup` | `{"url"}`: what a link to an open chat model holds. Answers `{host, site, owner, repo, url, license, gated, budget_mb, files: [{file, size, needs_mb, fits, checked, id, installed}], pick, skipped, links}`. See [Community models](#community-models). |
| `POST /v1/community` | `{"repo", "file"}`: add one of those files as a chat package and start its download. Answers `{"job", "id", "name"}`; follow the job like any other download. |
| `DELETE /v1/community/{id}` | Remove a community model: its files and its place in the list. A running download of it stops first. |
| `GET /v1/jobs`, `GET /v1/jobs/{id}`, `DELETE /v1/jobs/{id}` | Download progress, and cancel. A cancelled or interrupted download resumes where it stopped. |
| `GET /v1/engine`, `DELETE /v1/engine` | Which models are in memory (`loaded`: the one used last; `all`); unload them. A chat model and a picture or video model stay in memory together when both fit the budget. |
| `POST /v1/chat/completions` | OpenAI-compatible chat. `model` is a package id such as `steep-4b`. Streaming and tool calls work. |
| `POST /v1/images/generations` | `{"model": "crema-6b", "prompt", "size": "1024x1024", "steps", "seed"}`. The answer has the PNG as base64 and `image: {id, path, bytes}` for the saved file. |
| `GET /v1/images/progress` | Stage and steps of the picture being made (`{"progress": {stage, completed, total, elapsed_ms}}`), or null. Closing the request to `/v1/images/generations` stops the engine too. |
| `GET/DELETE /v1/images/files/{id}.png` | A saved picture. |
| `POST /v1/audio/speech` | OpenAI-compatible: `{"model": "whistle-82m", "input", "voice", "speed"}`. Answers with a WAV; Markdown in `input` is read as plain sentences. English. |
| `GET/DELETE /v1/audio/files/{id}.wav` | A saved reading (`x-rigbrew-audio` in the answer above says which). |
| `POST /v1/audio/transcriptions` | OpenAI-compatible, multipart: `file` (16-bit PCM WAV, any sample rate, up to 10 minutes), `model` (optional), `language` (optional, such as `tr`; the model tells otherwise). Answers `{"text", "language", "duration"}`. The recording is deleted when the answer is sent. |
| `POST /v1/videos/generations` | `{"model": "pour-14b", "prompt", "size": "640x384", "frames": 121, "seed"}` (sizes, lengths and steps come from the package's `defaults`). Starts a clip in the background and answers `202 {"job"}` at once. While a clip is being made, requests that would load another model are refused with 409. |
| `GET /v1/videos/jobs`, `GET/DELETE /v1/videos/jobs/{id}` | Progress (`stage`, `step` of `steps`, `elapsed_ms`) and, when done, `video: {id, path}`; DELETE stops it at the next step. |
| `GET/DELETE /v1/videos/files/{id}.webm` | A saved clip (VP8 WebM, 24 frames per second). |
| `GET/POST /v1/studio/projects`, `GET/PUT/DELETE /v1/studio/projects/{id}` | Studio films: a film is `rigbrew-film.json` and a `clips/` folder inside a folder the person chooses. `POST {folder, name}` starts or opens one; `PUT {name, size, shots: [{id?, prompt, frames}]}` saves it. A shot keeps its clip until its description, length or the film's shape changes. |
| `POST/DELETE /v1/studio/projects/{id}/render` | Make the shots that are not made yet (or `{"shots": [id]}`), one after another, with the installed video package; DELETE stops. |
| `POST /v1/studio/projects/{id}/storyboard` | `{idea, shots}`: the chat model writes that many shot descriptions. Nothing is saved. |
| `GET /v1/studio/projects/{id}/clips/{shot}.webm` | A made shot. |
| `POST /v1/agent/tasks` | Code: start a task on a folder. `{"folder", "prompt", "model", "mode", "web"}`; see [Code tasks](#code-tasks). Answers `{"task"}`. |
| `GET /v1/agent/tasks`, `GET /v1/agent/tasks/{id}` | Tasks, newest first (`?folder=` for one folder); one task with its state, its steps so far and what waits for an answer. |
| `POST /v1/agent/tasks/{id}/approve` | `{"call", "allow": true or false, "always": false}`: answer the step that waits. |
| `POST /v1/agent/tasks/{id}/message` | `{"text", "mode", "web"}`: continue a task that is not working with a follow-up. |
| `DELETE /v1/agent/tasks/{id}` | Stop a task where it is (a running command is killed). `?remove=true` also forgets it. |
| `GET /v1/agent/skills` | The skills a task can use (`?folder=` adds the folder's own): name, description, where it comes from. |
| `GET/DELETE /v1/chats`, `GET/PUT/DELETE /v1/chats/{id}` | Saved conversations. `PUT {title, data}`; `data` is the app's own JSON, 8 MB at most. |
| `GET/PUT/DELETE /v1/agents`, `GET/PUT/DELETE /v1/agents/{id}` | Agents the person made, kept in `agents.json` in the data folder. `PUT /v1/agents/{id}` takes `{name, instruction, tools, starter}`: a name of at most 60 characters, an instruction of at most 4000, `tools` from `generate_image`, `read_aloud` and `generate_video`, and an optional first message of at most 300. The id is 1 to 40 of `a-z`, `0-9` and `-`. `PUT /v1/agents` takes `{agents: [...]}` and replaces the list. |
| `GET /v1/artifacts/frame` | The empty page the app shows a chat artifact in. It allows inline styles and scripts and nothing from anywhere else, holds no data, and is the one address that needs no token. |
| `GET /v1/usage` | Totals for today, the last 7 days and all time, disk used per package, and what is waiting to be sent. |
| `GET /v1/activity?days=365`, `GET /v1/activity/recent` | Day-by-day activity and a readable list of recent actions (the profile page). |
| `GET/POST/DELETE /v1/consent` | The consent state; accept the current version; withdraw. |
| `POST /v1/events`, `GET /v1/events/pending` | Record a usage event; see exactly what is queued to be sent. |
| `POST /v1/sync` | Send queued usage data now. |
| `GET /v1/app/config` | Update and sponsor information from Rigbrew's server, cached for offline use. |

Example, one chat turn:

```sh
curl -s -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"model":"steep-4b","messages":[{"role":"user","content":"Say hello."}]}' \
  http://127.0.0.1:$PORT/v1/chat/completions
```

The first request for a model loads it into memory, which takes a few seconds. A chat model and a picture or video
model share memory when both fit the budget; otherwise asking for the other one unloads the first.

## Code tasks

A Code task is a chat model working on one folder with tools, in a loop that runs inside `rigbrewd`. It goes on when
the app shows another screen, and it is kept as one JSON file under `agent/` in the data folder, so finished tasks
are still listed after a restart (one that was working then is marked `interrupted`).

```sh
curl -s -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"folder":"/path/to/project","prompt":"Why does the test fail?","mode":"ask"}' \
  http://127.0.0.1:$PORT/v1/agent/tasks
```

A task has a `state` (`running`, `waiting`, `done`, `failed`, `stopped`, `interrupted`), `steps` and, while it
waits for the person, `pending`. A step is one of `{"kind": "user", "text"}`, `{"kind": "model", "text"}`,
`{"kind": "note", "text"}` or `{"kind": "tool", "call", "name", "arguments", "status", "result", "diff"}` with the
status `running`, `waiting`, `done`, `failed`, `denied` or `stopped`. `pending` is `{"call", "tool", "path",
"command", "diff"}`; answer it with `POST .../approve` and the same `call`.

| Mode | What it means |
|---|---|
| `ask` (default) | A change to a file and a command wait for the person. The change is shown as a unified diff. |
| `read_only` | The tools that change files or run commands are not offered to the model at all. |
| `auto_edits` | Changes to files inside the folder apply at once. Commands still wait. |

`"always": true` on a command allows that exact command line for the rest of the task; on a file change it turns
the task to `auto_edits`. A call the person said no to is not asked again until their next message, and after two refusals of one tool that tool is not asked for again until then.

The tools: `list_files`, `read_file`, `search`, `edit_file`, `write_file`, `run_command`, `use_skill`, `finish`,
and `web_search` and `fetch_url` when the task was started with `"web": true`.

- **Files stay inside the folder.** Every path is resolved through the file system first, so `..`, an absolute
  path and a symbolic link that leads out are refused alike. Nothing inside `.git` is written. Rigbrew's own data
  folder cannot be chosen, nor a folder that contains it.
- **Commands wait for a yes, and on macOS run in a sandbox.** `run_command` runs a shell command with the folder as
  its working directory, for two minutes at most. On macOS it runs under the system's sandbox: it can read what the
  person can, write only inside the folder and the temporary folders, and reach the network only when the task was
  given the web. On Windows and Linux it is not confined yet and can do whatever the person's account can.
- **Everything is capped.** A tool result is at most about 4,000 characters and says when it was cut; a task stops
  after 40 model turns for one request and says so.
- **Web tools leave the machine.** `web_search` sends the query to DuckDuckGo's HTML endpoint; `fetch_url` gets one
  http(s) page (public addresses only, checked again at every redirect; 15 seconds, 2 MB) and returns its text.
  They are off unless the task says `"web": true`.

A skill is a folder with a `SKILL.md`: YAML front matter with `name` and `description`, then instructions (the
open Agent Skills format). Skills are read from `skills/` in the data folder and from `.rigbrew/skills/` in the
task's folder; three come built in (`explain-codebase`, `fix-failing-test`, `write-readme`, in
[`daemon/skills`](../daemon/skills)). The model sees the names and descriptions and reads a skill's instructions
with `use_skill` when it needs them.

## What leaves the machine

Models run locally and prompts are never sent to a model provider. `rigbrewd` sends usage data to
`https://api.rigbrew.dev` in two layers, both defined in [`core/src/events.rs`](../core/src/events.rs) and
[`daemon/src/cloud.rs`](../daemon/src/cloud.rs):

- **Basic usage, always.** Event kinds, counts, sizes, speeds, errors, hardware class, app version and a random
  install id. Before it is stored, every payload is reduced by `strip_content`: no text people typed, no model
  output, no strings longer than 120 characters, no account id. The server strips it again on arrival.
- **Content, only with consent and an account.** The text of prompts and answers. Builds from this repository have
  no account module, so they never send content.

A Code task with web search allowed sends its search queries to DuckDuckGo and fetches the pages the model opens.
That is off by default and chosen per task. Usage events of Code tasks carry the tool's name, the decision and
counts, never a path, a command or file content.

`DO_NOT_TRACK=1` (or `RIGBREW_NO_TELEMETRY=1`) in the environment switches off every upload. `GET /v1/events/pending`
shows the queue at any time.

## The manifest and engines

The list of packages and engines is a JSON manifest signed with minisign (Ed25519). The public key is compiled into
`rigbrew-core` (`manifest::PUBLIC_KEY`, the same as [`registry/rigbrew-manifest.pub`](../registry/rigbrew-manifest.pub)).
By default `rigbrewd` loads `https://models.rigbrew.dev/manifest.json` and its `.minisig`, checks the signature, and
keeps the last verified copy for offline starts. Release builds refuse an unsigned manifest; a single changed byte
is always refused. A local file or another URL can be passed as the first argument for development, with a
`<file>.minisig` next to it (development builds also accept an unsigned file, with a warning).

Every file a package or engine lists carries its size and sha256; downloads are checked byte for byte before they
are used. Engine programs (llama.cpp's `llama-server`, stable-diffusion.cpp's `sd-server`, sherpa-onnx's
`sherpa-onnx-offline-tts`) are downloaded per platform from the same manifest, unpacked into `engines/` (plain files
only), and run as child processes; the servers bind to loopback. A package file with the role `bundle` is a folder
packed as a tar: after it is verified it is unpacked next to itself (plain files only) and the archive is deleted.

## Community models

A person can add an open chat model that is not in the manifest by pasting a link. These models are not tested by
Rigbrew and are never mixed into the signed manifest: they are kept in `community.json` in the data folder, their
files live under `community/<owner>__<repo>/`, and `GET /v1/packages` lists them after the manifest's packages with
`"community": true` and `source` saying where they come from (`name`, `url`, `license`). They are chat packages for
the llama.cpp engine with one version (`gguf`), so `POST /v1/chat/completions`, the jobs and `GET /v1/usage` treat
them like any other package. Their ids start with `community-`.

`POST /v1/community/lookup` accepts:

- a Hugging Face model, as `owner/name` or `https://huggingface.co/owner/name`, optionally with a file
  (`owner/name/file.gguf`, or a `/blob/` or `/resolve/` link to a `.gguf` file);
- a GitHub repo or release (`https://github.com/owner/name`, `/releases/tag/<tag>`, or a release file). The `.gguf`
  files of the release are listed. When there are none, `links` holds the Hugging Face models the README links to.

Only `https://huggingface.co` and `https://github.com` are accepted, with no port or sign-in in the link. Owner,
repo, revision and file names may hold letters, digits, `.`, `_`, `-` and `+`, never start with a dot, and file
names must end in `.gguf`. Files larger than 200 GB, parts of split models (`-00001-of-00003.gguf`) and vision
add-ons (`mmproj`) are left out and counted in `skipped`. Refused links answer 400, repos that are private or do not
exist 404, and a site that cannot be reached 502, each with a sentence in `error.message`.

A file needs its size and about 15% more in memory (`needs_mb`). `fits` compares that with `budget_mb`, the
accelerator's memory budget from `GET /v1/system` (the processor's on a machine without one). `pick` is the file the
link named when it fits, otherwise the largest file that fits. `POST /v1/community` asks the site again, so sizes and
checksums never come from the request; it answers 409 for a file that does not fit and for a repo that asks for a
sign-in on its site (`gated`). Hugging Face files are pinned to the commit the lookup saw. The download is checked
against the site's sha256 when it gives one (`checked`); otherwise only the size is checked.

```sh
curl -s -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"repo":"bartowski/SmolLM2-360M-Instruct-GGUF","file":"SmolLM2-360M-Instruct-Q4_K_M.gguf"}' \
  http://127.0.0.1:$PORT/v1/community
```

## Environment

| Variable | Effect |
|---|---|
| `RIGBREW_DATA_DIR` | Use another data folder (packages, chats, the event queue). |
| `RIGBREW_MANIFEST` | Manifest file or URL, instead of the default. |
| `RIGBREW_BASE_URL` | Download package files from another mirror (files are still checked against the signed manifest). |
| `RIGBREW_ENGINE_DIR` | Use engine programs from this folder instead of downloading them. |
| `RIGBREW_PORT` | Listen on a fixed port. |
| `RIGBREW_IDLE_MINUTES` | Take models out of memory after this many minutes without use (10; 0 keeps them). |
| `DO_NOT_TRACK`, `RIGBREW_NO_TELEMETRY` | Send nothing. |
