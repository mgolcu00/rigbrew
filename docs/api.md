# The local API

The app never runs a model itself. It talks to `rigbrewd`, a small service on the same machine, and `rigbrewd`
runs the engines. Anything that can speak HTTP can use the same API: the chat endpoint is OpenAI-compatible, so
existing clients work against your own machine.

## Connecting

`rigbrewd` listens on `127.0.0.1` only, on a random port, and writes the port and a random token to
`daemon.json` in the data folder (`~/Library/Application Support/Rigbrew` on macOS) with mode `0600`.
Every request needs `Authorization: Bearer <token>`.

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
| `GET /v1/jobs`, `GET /v1/jobs/{id}`, `DELETE /v1/jobs/{id}` | Download progress, and cancel. A cancelled or interrupted download resumes where it stopped. |
| `GET /v1/engine`, `DELETE /v1/engine` | Which model is in memory; unload it. |
| `POST /v1/chat/completions` | OpenAI-compatible chat. `model` is a package id such as `steep-4b`. Streaming and tool calls work. |
| `POST /v1/images/generations` | `{"model": "crema-6b", "prompt", "size": "1024x1024", "steps", "seed"}`. The answer has the PNG as base64 and `image: {id, path, bytes}` for the saved file. |
| `GET/DELETE /v1/images/files/{id}.png` | A saved picture. |
| `POST /v1/audio/speech` | OpenAI-compatible: `{"model": "whistle-82m", "input", "voice", "speed"}`. Answers with a WAV; Markdown in `input` is read as plain sentences. English. |
| `GET/DELETE /v1/audio/files/{id}.wav` | A saved reading (`x-rigbrew-audio` in the answer above says which). |
| `POST /v1/videos/generations` | `{"model": "pour-5b", "prompt", "size": "832x480", "frames": 49, "steps": 20, "seed"}`. Starts a clip in the background and answers `202 {"job"}` at once. While a clip is being made, requests that would load another model are refused with 409. |
| `GET /v1/videos/jobs`, `GET/DELETE /v1/videos/jobs/{id}` | Progress (`stage`, `step` of `steps`, `elapsed_ms`) and, when done, `video: {id, path}`; DELETE stops it at the next step. |
| `GET/DELETE /v1/videos/files/{id}.webm` | A saved clip (VP8 WebM, 24 frames per second). |
| `GET/DELETE /v1/chats`, `GET/PUT/DELETE /v1/chats/{id}` | Saved conversations. `PUT {title, data}`; `data` is the app's own JSON, 8 MB at most. |
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

The first request for a model loads it into memory, which takes a few seconds. One model is in memory at a time:
asking another model unloads the current one.

## What leaves the machine

Models run locally and prompts are never sent to a model provider. `rigbrewd` sends usage data to
`https://api.rigbrew.dev` in two layers, both defined in [`core/src/events.rs`](../core/src/events.rs) and
[`daemon/src/cloud.rs`](../daemon/src/cloud.rs):

- **Basic usage, always.** Event kinds, counts, sizes, speeds, errors, hardware class, app version and a random
  install id. Before it is stored, every payload is reduced by `strip_content`: no text people typed, no model
  output, no strings longer than 120 characters, no account id. The server strips it again on arrival.
- **Content, only with consent and an account.** The text of prompts and answers. Builds from this repository have
  no account module, so they never send content.

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

## Environment

| Variable | Effect |
|---|---|
| `RIGBREW_DATA_DIR` | Use another data folder (packages, chats, the event queue). |
| `RIGBREW_MANIFEST` | Manifest file or URL, instead of the default. |
| `RIGBREW_BASE_URL` | Download package files from another mirror (files are still checked against the signed manifest). |
| `RIGBREW_ENGINE_DIR` | Use engine programs from this folder instead of downloading them. |
| `RIGBREW_PORT` | Listen on a fixed port. |
| `DO_NOT_TRACK`, `RIGBREW_NO_TELEMETRY` | Send nothing. |
