# The local API

The app never runs a model itself. It talks to `rigbrewd`, a small service on the same machine, and `rigbrewd`
runs the engines. Anything that can speak HTTP can use the same API: the chat endpoint is OpenAI-compatible, so
existing clients work against your own machine.

## Connecting

`rigbrewd` listens on `127.0.0.1` only, on a random port, and writes the port and a random token to
`daemon.json` in the data folder (`~/Library/Application Support/Rigbrew` on macOS) with mode `0600`.
Every request needs `Authorization: Bearer <token>` (all but `/v1/artifacts/frame` and `/v1/media`, see below).

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
| `POST /v1/community/lookup` | `{"url"}`: what a link to an open chat model holds. Answers `{host, site, owner, repo, url, license, gated, budget_mb, files: [{file, size, needs_mb, fits, checked, id, installed, projector}], pick, skipped, links}`. See [Community models](#community-models). |
| `POST /v1/community` | `{"repo", "file"}`: add one of those files as a chat package and start its download. Answers `{"job", "id", "name"}`; follow the job like any other download. |
| `DELETE /v1/community/{id}` | Remove a community model: its files and its place in the list. A running download of it stops first. |
| `GET /v1/community/catalog` | The community models Rigbrew suggests, from a signed file: `{updated, stale, models: [{id, name, kind, by, url, license, about, status, added, note, more}]}`. `?refresh=1` asks for the file again now. See [The community catalog](#the-community-catalog). |
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
| `GET/DELETE /v1/videos/files/{id}.webm` | A saved clip (VP8 and PCM in WebM, 24 frames per second). On macOS the answer is its playable copy, `Content-Type: video/mp4`; see [Clips the app plays](#clips-the-app-plays). DELETE removes the clip and its copy. |
| `GET/POST /v1/studio/projects`, `GET/PUT/DELETE /v1/studio/projects/{id}` | Studio films: a film is `rigbrew-film.json` and a `clips/` folder inside a folder the person chooses. `POST {folder?, name}` starts or opens one (without a folder: a new folder named after the film in `Movies/Rigbrew`, which the list gives as `home`); `PUT {name, size, auto_export, fade_in, fade_out, master, shots: [{id?, prompt, frames, continues, trim_start, trim_end, join, join_seconds, volume, mute}], shot_track: {volume, pan, mute, solo}, tracks: [{id?, name, color, volume, pan, mute, solo, regions: [{id?, file, start, offset, length, gain, fade_in, fade_out, loops}]}]}` saves it. `fade_in` and `fade_out` fade the film for half a second at its start and end; `master` (0 to 2) is how loud the whole film is. Per shot: `join` is how the film goes into it (`cut`, `dissolve` or `black`; not used when the shot continues the one before) and `join_seconds` how long that takes (0.25 to 2, half a second unless set), `volume` 0 to 1.5 and `mute` are for its own sound. `shot_track` is the channel strip of the shots' own sound as a whole. A sound track holds regions: `file` is a sound file by its full path or by its name inside the film's folder (wav, mp3, m4a, aac, flac, ogg, opus, aiff; a file the film did not use before has to exist), `start` is seconds into the film, `offset` seconds into the file, `length` how long it plays from there, `loops` how many times in a row, `gain` 0 to 2, `fade_in` and `fade_out` seconds at its ends. Track `volume` is 0 to 2, `pan` -1 (left) to 1 (right) turns down the other side, `mute` leaves the track out and `solo` leaves every other strip out. At most 32 tracks of 400 regions. Ids are lowercase letters and digits; one that is missing or used twice is replaced. A shot keeps its clip until its description, length or the film's shape changes, and counts as made again when they are changed back. A shot with a clip that is taken out of the list comes back with its clip and takes when a later save names its id again (the last 40 are kept). A film from before sound tracks (`sound`, `sound_volume`, `sound_once`) gets one track named "Music" with one region when it is read. A film with `auto_export` is exported by the service when its last shot is made. The film's answer carries `export` (where its export is) and `estimate: {all, left}`: about how many seconds the shots that are not made will take, and the ones waiting or being made now. The times come from the shots already made on this machine (`studio-pace.json` in the data folder; before anything is measured, 25 seconds of work for a second of film, 80 when the shot continues another). The list carries `pace`: what a five-second shot takes here. |
| `POST/DELETE /v1/studio/projects/{id}/render` | Make the shots that are not made yet (or `{"shots": [id]}`), one after another, with the installed video package; DELETE stops. |
| `POST /v1/studio/projects/{id}/storyboard` | `{idea, shots, as_written?}` (up to 60): the chat model writes that many shots in scenes for the film as it is saved, `{prompt, continues, lines: [{who, text}]}` each: a scene starts with a cut and its other shots continue the shot before; the cast is named and never described; the look of the film is left out (both are added when a shot is made); with `speech` the shots get spoken lines (`who` is a cast member's id, or empty for the narrator). With `as_written` every line of the idea is one shot as it stands and the chat model is not asked. A long film is written in parts. Nothing is saved. Needs the video model, and the chat model unless `as_written`: `409` with one short sentence otherwise. |
| `POST /v1/studio/brief` | `{text}`: a film directed from a few plain words. One chat-model call answers a form (title, shape, length, look, narration, language, cast) that is read line by line, with a plain default for whatever is missing; a shape the person asks for in so many words ("for Reels") wins. Answers `{title, shape, size, seconds, frames, shots, look, speech, language, cast: [{name, look}]}` in the sizes and lengths the video model makes. Nothing is saved. `409` without the video or the chat model. |
| A film's cast, look and seed | Saved through `PUT`: `look` (one line every shot shares), `cast: [{id?, name, look, voice}]` (at most 12; names are unique), `seed` (every shot without its own is made with it; `null` takes it away). When a shot is made, the video model is told its description, then `Name: look.` for every cast member the description names (whole word, any case), then the film's look: word for word, the same in every shot. A shot whose told text changes (a look was edited, the film's look changed) is marked `changed`. A shot's `from` names a cast member whose portrait it starts from, when the video model starts clips from pictures. The film says in `cast_in` which shots name which member. |
| `POST /v1/studio/projects/{id}/cast/look`, `POST/GET .../cast/{member}/portrait` | `{name, words}`: the chat model writes a look from a few words (not saved). POST portrait: the picture model makes a portrait from the member's look at the film's shape, kept as `<folder>/cast/<member>.<n>.png`; GET answers it. `409` without the model, or while a shot is being made. |
| `POST/GET /v1/studio/projects/{id}/import` | `{path}`: brings a video file (mp4, mov, m4v, mkv, webm, avi) into the film with the packaged ffmpeg: cut where its scenes change (`select='gt(scene,0.3)'`), no shot shorter than 1 s, a scene longer than 8 s in equal parts, at most 120 shots a file. Each shot is written as H.264 and AAC at 24 frames a second, at the film's shape (filled and cropped), at twice the film's width and height when the video has the pictures for it, with its own sound. An empty film takes the shape closest to the video's. The shots are ordinary shots with one take (`ext: "mp4"`, `large`). GET says where it is: `{state, stage, done, of, shots, left_out, error}`. |
| Spoken lines, `POST/GET .../dub`, `POST/GET .../listen` | A shot's `lines: [{id?, who, text, said: {language: text}}]` are saved through `PUT`. `POST dub {language?}`: the installed voice reads every line (a voice per speaker: the member's `voice`, else the film's) into `<folder>/voice/`, and the readings become regions on a track of kind `voice` named after the language, at each shot's place, one after another. While a voice region plays, the shots' own sound goes down by the film's `duck` (0 to 1, 0.6 unless set), in the player and in the export. Another language is translated by the chat model first (kept in `said`); the voice reads English and Chinese, so any other language is translated for the subtitles only and the film is heard as it was written. `dub` in `PUT` chooses the language that is heard and read; the other voice tracks are muted. `POST listen {shots?}`: the installed listening model writes down what is said in the clips of shots without lines (`409` when none is installed). |
| Subtitles, `GET .../subtitles`, `PUT .../captions` | The film's `cues` are its lines in time, in the language shown: a line that was read is on screen while its reading plays, the others share their shot's time; rows of 42 letters, two at a time. An export always writes `<name>.srt` next to the film when there are lines, and with `subtitles.show` a `mov_text` track named by its language. With `subtitles.burn` the subtitles are drawn into the pictures: the packaged ffmpeg has no text drawing, so the app draws each subtitle on a canvas and hands it over (`PUT captions?key=&n=&of=&w=&h=`, raw RGBA or `{rgba: base64}`); the export lays them over the film. Without those pictures (an export that starts by itself) the track is written instead. The export's state says which: `subtitles: none, file, track or burned`. |
| `POST /v1/studio/projects/{id}/narration` | `{text, at?, track?}`: the installed voice package reads it; the WAV is saved in the film's folder (`narration.wav`, `narration 2.wav`, ...) and becomes one region at `at` seconds on the track `track`, or on the track named "Narration", which is made when there is none. Answers with the film and `region: {track, id, seconds}`. |
| `POST/GET /v1/studio/projects/{id}/export` | Join the made shots, with their trims, joins and volumes, and mix the sound tracks into them, into `<folder>/<name>.mp4` (H.264 and AAC, 24 frames per second). A film of plain cuts without tracks is joined as a list; dissolves, fades, volumes and tracks go through a filter graph: every region at its place, repeated, faded, at its gain and side, with muted tracks left out and soloed ones alone, the master volume last and the peaks held under full level. The film is as long as its pictures; sound after them is cut. A sound file that is gone stops the export with a message that names it. The film's `export: {scale, quality, name}` (saved through PUT, or given in the POST for one export) says how the file is written: `scale` 2 enlarges the picture to twice its width and height with Lanczos (no detail is added), `quality` is `small`, `balanced` or `best` (3.4, 7.9 or 15.7 Mbit/s at 640x384), `name` is the file's name without its ending. `{music, music_volume}` in the POST lays one more sound file under the whole film for that export. The answer of a finished export has `file`, `bytes` and `seconds`. GET says where the export is: `{state: none, working, done or failed, file, error}`. |
| `POST /v1/studio/projects/{id}/reveal` | Show the exported file in the system's file manager (the film's folder when there is no file yet). |
| `POST /v1/studio/projects/{id}/media` | `{}` for the exported film, `{shot, take?}` for a clip (its playable copy): a short-lived address the app's `<video>` plays it from, `{url: "/v1/media?token=…", type, bytes}`. `404` before the first export. The film's answer carries `film: {file, bytes, modified_ms, current}` (null before the first export): `current` is true when no edit came after what the file was made from. See [Playing the whole film](#playing-the-whole-film). |
| `GET /v1/studio/projects/{id}/sound?file=` | A sound file of the film, so the app can play it: one in the film's folder, one added to its library, or one a region plays (up to 200 MB). Nothing else is read. |
| `GET/POST/DELETE /v1/studio/projects/{id}/sounds` | The film's library of sounds: `{sounds: [{file, name, bytes, from: folder, added or used, missing}]}`, the sound files in its folder and one folder down, and the ones added from elsewhere. `POST {path}` adds a file, or every sound file directly in a folder; `DELETE ?file=` takes an added one out of the list (the file stays). |
| `GET /v1/studio/projects/{id}/peaks?shot=` or `?file=` | The small picture of a sound for the timeline: `{rate: 50, seconds, peaks: [low, high, low, high, ...]}`, the lowest and highest value of every fiftieth of a second, each from -127 to 127, of the clip of a shot (the take in use) or of a sound file of the film. Read once with FFmpeg and kept next to the clips as `.peaks.json`. A clip without sound has no peaks. |
| `POST /v1/studio/projects/{id}/shots/{shot}/rewrite` | `{hint?, prompt?}`: the chat model writes the description of one shot again, in the words of the shots around it (`prompt` is the description as it stands when it is not saved yet, `hint` what should be different). Answers `{prompt}`; nothing is saved. |
| `POST /v1/studio/projects/{id}/shots/{shot}/take`, `DELETE .../shots/{shot}/takes/{n}` | Takes: making a shot again keeps the clip before as a take (the last four; `takes: [{n, seed, seconds, frames, made_frames}]` and `take`, the one in use, on the shot). `POST {take: n}` puts a take in use; DELETE removes one and its clip. A shot's `seed`, when set through PUT, is used for its next clip: the same seed and description give the same clip again. |
| `GET /v1/studio/projects/{id}/clips/{shot}.webm`, `.../clips/{shot}.jpg` | A made shot (the take in use; `{shot}.{n}.webm` and `{shot}.{n}.jpg` for one take), and one small picture of it (from its middle) for the timeline. A shot carries `made_frames`: the frames its clip really has. A made shot that continues another is marked `changed` when a save puts it behind a different shot. On macOS a made clip is answered as its playable copy (`video/mp4`); a clip brought in from a video is an MP4 already. See [Clips the app plays](#clips-the-app-plays). |
| `POST /v1/agent/tasks` | Code: start a task on a folder. `{"folder", "prompt", "model", "mode", "web", "connectors", "images"}`; see [Code tasks](#code-tasks) and [What the person sends](#what-the-person-sends). With `{"film", "prompt", "model", "mode", "selection"}` instead of a folder: a conversation of Studio's assistant about a film; see [Studio's assistant](#studios-assistant). Answers `{"task"}`. |
| `GET /v1/agent/tasks`, `GET /v1/agent/tasks/{id}` | Tasks of Code, newest first (`?folder=` for one folder; `?film=` lists the assistant's conversations about one film instead); one task with its state, its steps so far and what waits for an answer. |
| `PUT /v1/agent/tasks/{id}` | `{"title"}`: the person names the task (80 characters at most). Answers `{"task"}`; see [Titles](#titles). |
| `POST /v1/agent/tasks/{id}/approve` | `{"call", "allow": true or false, "always": false}`: answer the step that waits. |
| `POST /v1/agent/tasks/{id}/message` | `{"text", "images", "continue", "mode", "web", "connectors", "selection"}`: continue a task that is not working with a follow-up, or, while it works, leave the message for its next turn (`queued`). `"continue": true` with no text goes on where a stopped task was. `selection` only for a film. |
| `DELETE /v1/agent/tasks/{id}/queue/{q}` | Take back a message that waits for the next turn. `409` once it went in. Answers `{"task"}`. |
| `GET /v1/agent/files?folder=&q=` | The files and folders of a folder whose path matches `q` (its letters in order), best first, at most 20, to name one with `@`. Folders end with `/`. Answers `{"files"}`. |
| `POST /v1/agent/tasks/{id}/undo` | Take back what the newest request changed in the folder; see [Undo](#undo). Answers `{"task", "restored", "removed"}`. `409` while the task works or when nothing is left to take back. |
| `DELETE /v1/agent/tasks/{id}` | Stop a task where it is (a running command is killed). `?remove=true` also forgets it. |
| `DELETE /v1/agent/tasks/{id}/app` | Stop the app the task started with `start_app`; see [Run, look, check](#run-look-check). Answers `{"task"}`. |
| `POST /v1/agent/look` | `{"task"?, "url"?, "size"?, "do"?}`: look at a page on this computer the way `look_at_page` does. With `task` the address defaults to the task's running app and what was seen is kept as the task's `page`; without one the answer also has the `text` and the `picture` (PNG, base64). Any other address is `400`. Answers `{"page"}`. |
| `POST /v1/agent/tasks/{id}/device` | Take a new picture of the phone's screen (the booted simulator, or the Android device `adb` sees) and keep it as the task's `device`. `409` when there is none. Answers `{"task"}`. |
| `GET /v1/agent/tasks/{id}/shots/{name}.png` | A picture the task took: the `image` of a step, the `shot` of its `page` or `device`. |
| `GET /v1/agent/looks/next`, `POST /v1/agent/looks/{id}` | For the app, not for people: it waits here (up to 20 seconds) for a page the service wants opened, `{"look": {"id", "url", "width", "height", "actions"}}` or `{"look": null}`, and hands back `{url, title, text, errors, failed, picture?, note}`. |
| `GET /v1/agent/skills` | The skills a task can use (`?folder=` adds the folder's own): name, description, where it comes from; with a folder also `instructions`, the project's instruction files the model reads (see [Project instructions](#project-instructions)). |
| `GET /v1/skills`, `POST /v1/skills` | Every skill with `source` (`built-in`, `yours`, `folder`), `enabled` and `where` (`any`, `code`, `chat`); `?folder=` adds a project's own. `POST {name, description, body, copy_of?}` makes one of the person's own; see [Skills](#skills). |
| `GET/PUT/DELETE /v1/skills/{name}` | One skill with its whole instructions, its licence line and its other files. `PUT {"enabled": false}` switches it off; `PUT {name, description, body}` changes it (a built-in one becomes the person's own version under the same name). `DELETE` removes the person's own. |
| `POST /v1/skills/{name}/use`, `GET /v1/skills/{name}/file?path=` | What the chat's `use_skill` and `read_skill_file` answer: `{"ok", "text"}`. |
| `POST /v1/skills/import` | `{"from": "folder" or "github", "location"}` looks at the skills there and answers `{"found": [{name, description, licence, bytes, files, problem}]}`; with `"save": [names]` it saves those. |
| `GET /v1/connectors`, `POST /v1/connectors` | Connectors the person added, each with its state, plus the ready-made ones and whether `npx` and `uvx` are installed. `POST` adds one; see [Connectors](#connectors). |
| `GET/PUT/DELETE /v1/connectors/{id}` | One connector, never with its secrets. `PUT` changes it and starts a running one again when what it runs changed; `DELETE` stops and removes it. |
| `POST /v1/connectors/{id}/start`, `POST /v1/connectors/{id}/stop` | Start it (again), or stop it. The answer is the connector with `state` and, when it did not start, `error`. |
| `GET /v1/connectors/{id}/tools` | The tools of a running connector: `name`, `description`, `params`, the tool's own `schema`, and `always`. |
| `POST /v1/connectors/{id}/call` | `{"tool", "arguments"}`: call one tool. Answers `{"ok", "text"}`. With `"from": "chat"` the text is worded for a model. |
| `POST /v1/connectors/{id}/allow` | `{"tool", "always": true or false}`: let a tool run without asking the person first, or ask again. |
| `POST /v1/connectors/find` | `{"query"}`: the three tools that fit best across the running connectors, as `matches` and as `text` for a model. |
| `GET/DELETE /v1/chats`, `GET/PUT/DELETE /v1/chats/{id}` | Saved conversations. `PUT {title, title_by, data}`; `data` is the app's own JSON, 8 MB at most. `title_by` says who wrote the title (`words`, `model` or `person`); left out, the chat keeps what it has. See [Titles](#titles). |
| `POST /v1/title` | `{"text", "answer", "model", "load"}`: a name for a conversation that began with `text`. Answers `{"title", "by"}`; see [Titles](#titles). |
| `GET/PUT/DELETE /v1/agents`, `GET/PUT/DELETE /v1/agents/{id}` | Agents the person made, kept in `agents.json` in the data folder. `PUT /v1/agents/{id}` takes `{name, instruction, tools, starter, connectors, skills}`: a name of at most 60 characters, an instruction of at most 4000, `tools` from `generate_image`, `read_aloud` and `generate_video`, an optional first message of at most 300, `connectors` (may it use connectors; off when left out) and `skills` (up to 12 names of skills pinned to it). The id is 1 to 40 of `a-z`, `0-9` and `-`. `PUT /v1/agents` takes `{agents: [...]}` and replaces the list. |
| `GET /v1/artifacts/frame` | The empty page the app shows a chat artifact in. It allows inline styles and scripts and nothing from anywhere else, holds no data, and needs no token. |
| `GET /v1/media?token=` | A file the service made, for the app's own `<video>`, which cannot send the bearer token: the token in the address is for one file. See [Playing the whole film](#playing-the-whole-film). |
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

## Clips the app plays

The video engine writes WebM: VP8 pictures and 16-bit PCM sound. The app's webview on macOS is WebKit, which
plays neither PCM in WebM nor decodes it for Web Audio. So on macOS every made clip gets a playable copy next to it,
`<stem>.play.mp4` (`clips/<shot>.<take>.play.mp4` in a film, `videos/<id>.play.mp4` for a chat's clip): H.264 from
the hardware encoder (`h264_videotoolbox`, constant quality 85, a keyframe every half second), AAC at 48 kHz stereo,
192 kbit/s, with its index at the front. It holds the same frames as the clip and measures about 48 dB PSNR against
it. The copy is made right after the clip (Studio's queue, a chat's clip), and for a clip from before on the first
request for it. One copy of a file is made at a time (a second request waits for it), two files at most at once,
written under a temporary name and renamed. A copy older than its clip is made again. When it cannot be made, the
clip is answered as it is.

The WebM stays the master: an export, the waveform, the last picture a continuing shot starts from and the
listening model read it, so nothing in a film goes through a second encoding. Deleting a take or a chat's clip
removes its copy; forgetting a film removes the copies in its `clips/` folder and leaves the clips.

On Windows the webview is Chromium and on Linux WebKitGTK plays through GStreamer (`matroskademux`, `vp8dec`, raw
PCM); both play the WebM as it is, and no copy is made there.

### Playing the whole film

An exported film is a file of 100 MB and more, and a `<video src>` cannot send the bearer token. So the app asks
`POST /v1/studio/projects/{id}/media` for an address, and its `<video>` plays `GET /v1/media?token=…` by itself:
the service answers `Range` requests (`206` with `Content-Range`, `Accept-Ranges: bytes`, `416` past the end), so
the film starts at once and is never read into memory as one piece. Clips stay small and are fetched with the token
as before, through object URLs, so Web Audio can mix their sound.

A token is two random UUIDs (64 hex digits), for one file the service resolved itself (a film's export, a clip or its playable
copy), never for a path the app sends. It goes after 20 minutes without a request, or after 12 hours; the app asks
for a new one each time the player opens, and once more when an address stops answering. `/v1/media` answers only
when it is asked for by `127.0.0.1`, `localhost` or `[::1]`, so a page that points a name of its own at this
machine gets `403`. The app's content security policy lets media load from `http://127.0.0.1:*` and nowhere else.

## Titles

A chat, a Code task and a film start with their first words as their name. The chat model writes a better one
when that costs nothing: `POST /v1/title` asks it for 3 to 6 words in the language of the conversation, with
thinking off, and cleans what comes back (one line, no quotes, no punctuation at the end, 48 characters at most).
`by` in the answer is `model` then. It is `words`, and `title` is the first words of `text`, when no chat model is
installed, when the answer was no use, and whenever asking would mean moving a model: the chat model is asked only
when it is in memory already, or when nothing is in memory and no clip, reading or recording is being made
(`"load": false` leaves out that second case). Nothing of a title request is recorded as an event.

`title_by` on a chat and on a task says who wrote its name: `words`, `model` or `person` (empty on one saved
before this was kept). The service names a new task itself once its first turn is over, from the request and the
start of the answer or the plan. The app asks for a chat's name after its first answer and saves it with
`PUT /v1/chats/{id}`. A name the person typed (`person`) is never replaced.

## Code tasks

A Code task is a chat model working on one folder with tools, in a loop that runs inside `rigbrewd`. It goes on when
the app shows another screen, and it is kept as one JSON file under `agent/` in the data folder, so finished tasks
are still listed after a restart (one that was working then is marked `interrupted`).

```sh
curl -s -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"folder":"/path/to/project","prompt":"Why does the test fail?","mode":"ask"}' \
  http://127.0.0.1:$PORT/v1/agent/tasks
```

A task has a `state` (`running`, `waiting`, `asking`, `done`, `failed`, `stopped`, `interrupted`), `steps` and,
while it waits for a yes or no to one step, `pending`. A step is one of `{"kind": "user", "text"}`,
`{"kind": "model", "text"}`, `{"kind": "note", "text"}`, `{"kind": "question", "text"}` (see
[Pace](#pace)), `{"kind": "pictures", "images"}` (the pictures of the person's message before it),
`{"kind": "thought", "text", "ms"}` (what the model thought before it answered, and for how long; see
[Thinking](#thinking)) or `{"kind": "tool", "call", "name", "arguments", "status", "result", "diff", "image"}` with the
status `running`, `waiting`, `done`, `failed`, `denied` or `stopped`; `image` names the picture a step took. A task
also has `queued` (messages that wait for the next turn, `{id, text, images}`) and `context`: how full the model's
context was at its last turn, `{used, size}` in tokens, by the engine's own count. `pending` is `{"call", "tool", "path",
"command", "diff"}`; answer it with `POST .../approve` and the same `call`.

A task in a list is `{"id", "folder", "title", "title_by", "model", "mode", "state", "created_ms", "updated_ms",
"request_ms", "steps"}` (`steps` is how many there are).

| Mode | What it means |
|---|---|
| `ask` (default) | A change to a file and a command wait for the person. The change is shown as a unified diff. |
| `read_only` | The tools that change files or run commands are not offered to the model at all. |
| `auto_edits` | Changes to files inside the folder apply at once. Commands still wait. |

`"always": true` on a command allows that exact command line for the rest of the task; on a file change it turns
the task to `auto_edits`. A call the person said no to is not asked again until their next message, and after two refusals of one tool that tool is not asked for again until then.

The tools: `list_files`, `read_file`, `search`, `edit_file`, `write_file`, `run_command`, `update_plan`,
`outline`, `use_skill`, `finish`, `start_app`, `look_at_page`, `check_project` and `simulator` (see
[Run, look, check](#run-look-check)), `read_skill_file` when a skill has other files, `web_search` and `fetch_url` when
the task was started with `"web": true`, and `find_tool` and `call_tool` when it was started with
`"connectors": true` and a connector is running. A `call_tool` that waits shows as `pending` with
`tool: "call_tool"` and the connector's name in `path`; the tool and its arguments are in the step.
`"always": true` on it allows that tool for the rest of the task.

A task also has a `plan`: the steps the model wrote with `update_plan`, each `{"text", "status"}` with the status
`pending`, `doing` or `done`, and at most one step `doing`. The plan is sent back to the model at the end of the
system message on every turn, so it is still there when older messages were left out to fit the context. A plan
that is finished is cleared by the person's next message.

`list_files` and `search` see the folder the way git does: what `.gitignore`, `.ignore` and git's own exclude files
name is left out, in a repository or not, and so are `.git`, `node_modules`, `target` and a few more build folders
even when nothing names them. Hidden files are kept. A file or folder named outright is still read. `outline` gives
the definitions of a source file with their line numbers (functions, classes, types, exported constants; the
methods one level in), or the top-level ones of each source file in a folder. It works with a few regular
expressions per language, not a parser, for Rust, Python, JavaScript, TypeScript, Go, Swift, Kotlin, Java, C, C++
and Svelte.

A command that failed and wrote more than `run_command` shows (4,000 characters) is read the way `check_project`
reads a check: the model gets the first line, the failures found in the output with their file and line (for pytest
each failing test with the first thing it said went wrong), and the end of the output, instead of its start and its
end.

After a change to a file went in, the result the model reads ends with how to check it: `check_project` when the
project has checks that can run here, otherwise the command that checks this project
(`cargo test`, the `test`, `check` or `build` script of `package.json` with the package manager of the lock file,
`python3 -m pytest` or `python3 -m unittest`, `go test ./...`, the `test`, `check` or `build` target of a
`Makefile`). When the final answer says that something works, passes or is fixed, and no command ended well
after the last change of that request, the task gets a `note` step under the answer: "Not checked: no test or
build ran after the last change." What `check_project` would run is also named at the end of the system message, and
the model is told that commands already run inside the folder. A `check_project` that passed counts like a command
that ended well.

### Run, look, check

Four tools let a task see its own work (`daemon/src/agent/see.rs`).

**`start_app {command?}`** starts the project's app as a background process the task owns and waits, up to 90
seconds, until it answers on this computer or dies. Without `command` the service reads the folder: the `dev`,
`start`, `serve` or `preview` script of `package.json` (with the port passed the way that tool takes it),
`manage.py`, a Flutter web app, `cargo run`, `go run .`, or `python3 -m http.server` for a folder with an
`index.html`. The service picks a free port: `{port}` in the command is replaced by it and `PORT` is set. A server
that chose another port and printed `http://localhost:<port>` is found by that line. The result is the address and
the last lines of output; a process that ended, or never answered, is a failed step with its output. One task has
one app. It is stopped when the task is stopped, fails or is removed, from the preview (`DELETE .../app`), when
`start_app` runs again, and when the service stops; a request that finished keeps it, so the person can look. At
most four run at once, and the oldest of a task that is not working makes room. `GET /v1/agent/tasks/{id}` shows
it as `app: {url, port, command, alive, started_ms, lines}`, or `null`. Not offered in `read_only`; in `ask` and
`auto_edits` the command waits for the person like `run_command`. A `run_command` that would start a server
(`python3 -m http.server`, `npm run dev`, `vite`, `flask run` and the like) is not run while `start_app` is offered:
the model is told to call `start_app`. A `command` that is only a port is taken as none. On macOS it runs in the command sandbox, which
lets it listen on this computer's own address and nothing more; the task's other commands may then talk to that
one port.

**`look_at_page {url, size?, do?}`** opens a page of the running app and gives its title, the text on it, the
errors its scripts wrote (`console.error`, errors nothing caught, rejected promises), the requests that failed, and a
picture. Only `http://localhost`, a name under `localhost`, `127.0.0.0/8` and `[::1]` are opened; anything else is
refused before anything is sent. `size` is `desktop` (1280 wide), `tablet` (820) or `phone` (390). `do` is up to ten
steps done before the look, `{"click": "text or CSS selector"}` or `{"type": "CSS selector or label", "text"}`.
The service has no browser, so the app lends its own: it waits at `GET /v1/agent/looks/next`, opens the address in
a hidden window that is named in no capability (the page in it can call nothing of the app, may only go to
addresses on this computer, and keeps no storage between looks), and posts what it saw. The picture is taken on
macOS; on Windows and Linux the result has the text and the errors and says that there is no picture. When no app
window is waiting, the HTML is fetched and read instead and the result says "not rendered". The picture is kept
under `agent/shots/<task>/` (the newest 30), named in the step's `image`, and, when the task's model has a vision
part, sent to the model for its next three turns as a message after the tool result:
`{"role": "user", "content": [{"type": "text", ...}, {"type": "image_url", "image_url": {"url": "data:image/png;base64,..."}}]}`.
The task's `page` is the last look: `{url, title, errors, failed, rendered, shot, note, at_ms}`. Every mode has this
tool, and it never asks.

**`check_project {what?}`** runs the project's own checks one after the other and answers in a few lines: what ran,
what passed, and the first eight failures of each with file and line, never the whole log. `what` is `fast` (the
default: the type check or build, the linter, the unit tests), `ui` (tests that drive the app) or `all`.

| The folder has | fast | ui |
|---|---|---|
| `Cargo.toml` | `cargo check --all-targets --message-format short`, `cargo test` | |
| `go.mod` | `go vet ./...`, `go test ./...` | |
| `package.json` | the `typecheck`, `type-check`, `check` or `tsc` script, else `tsc --noEmit` with a `tsconfig.json`; the `lint` script; the `test` script, else `node --test` | the `test:e2e`, `e2e`, `test:ui`, `test:playwright` or `test:cypress` script, else Playwright or Cypress when their config file is there |
| Python | `ruff check` and `mypy .` when they are configured; `pytest` or `unittest`, with the project's own `.venv` or `venv` when it has one | |
| `Package.swift` | `swift build`, `swift test` | |
| `pubspec.yaml` | `flutter analyze` and `flutter test` (or `dart`) | `flutter test integration_test` |
| an Xcode project | | named, and not run: the scheme and the device are the model's to give, with `run_command` |
| only a `Makefile` | `make check`, `make test` | |

Nothing is ever installed. A check whose program or packages are missing is "not run", with what is missing. The
first line is `Checks: 14 passed, 2 failed` (tests where the runner counts them, one per check otherwise). The
permissions are those of `run_command`, asked once for the whole list.

**`simulator {action, device?, app?}`** is offered when the folder holds a phone app and this computer has Xcode's
simulator (macOS) or `adb` on its PATH. `list` names the booted simulators and connected devices, `screenshot`
takes a picture of one, `logs` gives its last log lines (those of `app` when named, the errors otherwise). These
are fixed commands the service runs itself (`xcrun simctl list devices booted -j`, `simctl io <udid> screenshot`,
`simctl spawn <udid> log show --last 1m`, `adb devices -l`, `adb exec-out screencap -p`, `adb logcat -d -t 200`):
the device must be one of those listed and an app's name holds only letters, digits, dots, dashes and underscores.
They only read, so every mode has them and none asks. Building and starting the app is the model's own work
through `run_command`. The task's `device` is the last picture: `{shot, name, at_ms}`.

**Checking is not left to the model.** When a request changed files, the project has fast checks that can run
here, and no `check_project` or command ended well after the last change, the first `finish` is answered with
"call check_project now" (or, after a check that failed, "fix what it reported") instead of ending the request.
This is the same single answer as the last look of [a long task](#a-task-runs-until-it-is-done): the two are one
message, given once. A check the person said no to is not asked for again. Failing checks that come back the same
are a sign of being stuck like any other ([Pace](#pace)).

While the model writes, `GET /v1/agent/tasks/{id}` has the text so far in `writing` (the engine streams to the
service; the text becomes a `model` step when the turn ends, and `writing` is empty again). To follow it, ask with
`?brief=true` a few times a second: that answer has only `id`, `state`, `updated_ms`, the number of `steps` and
`writing`, and `updated_ms` says when to ask for the whole task again.

### What the person sends

- **Commands.** A message that starts with one of these is sent to the model as the request it stands for; the step
  keeps what was typed (`daemon/src/agent/project.rs`). Words after the command are added to it.

  | Command | The request |
  |---|---|
  | `/init` | Explore the folder and write `AGENTS.md` (or improve the instruction file that is there), and change nothing else. |
  | `/review` | Review the uncommitted changes: `git status` and `git diff HEAD` (at most 9,000 characters) go with the request, and new files are named. Outside a repository, or with nothing changed, the answer is `400` and nothing is sent. |
  | `/test` | Run the project's checks and fix what fails, until they pass. |
  | `/explain [what]` | Explain the codebase, or what is named, without changing anything. |
  | `/<skill>` | The skill's instructions are read for the model (a `use_skill` step) before the work starts; the words after it are the request. |

  `/clear` (a new task in the same folder), `/model` and `/mode` are the app's own and never reach the service. A
  `/` followed by anything else is sent as it is.
- **`@path`.** A word that starts with `@` and names a file or folder in the task's folder brings its text (at most
  4,000 characters a file, 12,000 together, six files) or, for a folder, its listing, after the message. The step
  shows only what was typed. `GET /v1/agent/files` is what the app's menu asks.
- **Pictures.** `images` holds up to four PNG files, base64 (a `data:` address is fine), of at most 8 MB each. They
  are kept with the task's other pictures and shown as a `pictures` step. A model with a vision part is sent the
  newest four as pictures with the message (`image_url` parts); one without is told that pictures came and that it
  cannot see them.
- **While it works.** A message sent to a task that is working (or waiting for a yes) is not refused: it waits in
  `queued` (at most eight) and goes into the conversation at the next turn, after the result of the step in progress
  and before the model is asked again. When the request ends first, the messages that waited start the next one. A
  stopped task keeps them; they go in with the next message or `continue`.
- **Continue.** `{"continue": true}` on a stopped, cut-off or failed task goes on from where it was: the model is told
  that the person asks it to go on.

### Project instructions

`AGENTS.md`, `RIGBREW.md` or `CLAUDE.md` in the task's folder, and in each folder above it up to the top of its git
repository, are put into the system message on every turn, the outer ones first so the nearest has the last word.
The same text under two names counts once; the whole is cut at 3,000 characters, and the nearest file keeps its room
first. The person's words come first when they disagree. `/init` writes `AGENTS.md`.

### Context and thinking

A chat model of 8 GB or more runs with a context of 32,768 tokens, a smaller one with 8,192. The conversation a task
sends grows with it: about 12,000 characters at 8,192 tokens and four times that at 32,768, and one answer may take
3,072 or 9,216 tokens. What does not fit becomes notes ([A task runs until it is done](#a-task-runs-until-it-is-done)).

#### Thinking

A model with the long context is asked to think before it answers (`enable_thinking`, and a medium
`reasoning_effort` for templates that take one); one with the short context is not, and is asked for a low effort:
measured on the Code tasks, the small model's thinking came out of the 3,072 tokens one answer may take, and the files it
wrote were cut off until the task failed. Some models
think either way. What the engine hands back as `reasoning_content` becomes a `thought` step before what the model
did next, with the time from its first word of thinking to its first word of answer; it is not sent back to the
model.

### How the app shows a task

The Code screen reads like a terminal agent (`app/src/lib/code-view.ts`): one compact line per thing done. A change
is a line with the file and the lines added and removed, a command is a line with the command; both open to the
diff or the output. An app that was started, a page that was looked at and the checks are one line each ("Started
the app on :5173", "Looked at /", "Checks 14 passed, 2 failed"), with the picture inside the opened step. When the
task has a running app or a picture of a phone's screen, a preview stands to the right of the work: the app in a
sandboxed frame at a phone's, a tablet's or a desktop's width with reload, open in the browser and stop, the
errors of the last look under it, and the phone's screen with a button for a new picture. A run of look-ups (`list_files`, `read_file`, `search`, `outline`, `web_search`, `fetch_url`,
the skill tools) is a single line, "Searching…" while it goes and "Read 4 files · Searched 2 times" afterwards,
without the words that were searched for; it opens to the steps. A change, a command, a question or an answer ends
the run. `update_plan` is no line: the plan is a checklist above the work. While the task is `running`, one line
above the box to type in shows a word from `app/src/lib/code-words.ts` that follows what is happening, the time
since `request_ms`, how full the context is ("34% of context"), and that Esc interrupts. One press stops the work
where it is; Continue takes it up again. What the model thought is a line "Thought for 12 s" that opens to the text,
except before a run of look-ups. The box to type in opens a menu for `/` at its start (the commands and the skills)
and for `@` (the files of the folder); a picture can be pasted or dropped into it. While the task works, what is sent
waits above the box as "Queued" and can be taken back.

### Pace

A small model left alone searches the same thing again and again. The service keeps count, so the rules hold
whatever the model does (`daemon/src/agent/pace.rs`):

- **A search is made once.** A `web_search` with the same words as an earlier one in the task (case, order and
  punctuation aside) is not sent: the result from before comes back with a line that says so. The same goes for a
  page that `fetch_url` already opened.
- **A budget for each request.** One message of the person gets 6 web searches and 12 pages. After that the tool
  is no longer offered to the model, the system message says so, and a call made all the same is answered with
  "enough has been searched" and not sent. The next message of the person starts the count again.
- **A search engine that does not answer.** A results page with no results that does not say "no results" (the
  engine turns automated searches away at times) is an error, not an empty answer. After two of them in a row the
  search is switched off for the request, and the model is told to open a page it knows or to answer and say what
  it could not check.
- **Results are small.** A search gives five results: title, address and one line of at most 160 characters. A
  page is cut to its readable text and its first 4000 characters, about an eighth of the model's context.
- **A nudge in the result.** The third web search in a row with no other kind of step carries a line that says to
  open a page or answer.
- **Signs of being stuck.** The same call with the same result a third time, twelve look-ups in a row (reading,
  listing, searching, the web) with nothing changed or run in between, or one tool failing three times in a row.
  The first two signs add a short nudge to the result the model reads. From the third on it is told to take stock:
  update the plan and go on in a different way, or finish with what it has. The sixth ends the request with one
  question for the person, as a `question` step, and the task's state is `asking`: it goes on with their next
  message. A change or a command that gives something new is progress and starts this ladder again, so tests that
  fail, a fix, and tests that fail in another way can go on for as long as it takes; reading a file again after a
  change is not a repeat. The same call three times in a row is not run a third time.

### A task runs until it is done

There is no limit on the steps of a task. What keeps a long one going (`daemon/src/agent/recall.rs` and the loop in
`daemon/src/agent.rs`):

- **Notes instead of a full history.** The conversation is sent whole while it fits the model's context (about
  12,000 characters of it at 8,192 tokens; see [Context and thinking](#context-and-thinking)). When it does not, the oldest messages leave it and what they held is kept as a short
  running note, written by the service from the calls and their results with no extra request to the model: the
  files read and what they define, the changes made, the commands run and how they ended, what the person asked
  along the way. The same file or command is one note that says how often. About three quarters of the budget
  stay as whole messages, so the last files read are still in front of the model; when the notes grow past a
  fifth of it the oldest look-ups go first. The first request of the person is kept word for word and sent first,
  and the plan is in the system message on every turn.
- **An answer that stops halfway is sent back.** Text with no tool call is the answer, except when it is empty,
  when the engine cut it off, or when the plan still has steps to do and the text is not a question: then the
  model is told so and the work goes on, at most twice in a row. `finish` with steps of the plan undone is
  answered with what is left, at most twice.
- **A last look.** After ten turns or more in which files were changed, the first `finish` is answered once with
  the request word for word and with the files it names that are not in the folder. Work that was not checked is
  said in the same answer, after short work too; see [Run, look, check](#run-look-check).
- **An engine error is tried again.** The model may be loading again or the engine busy: the request is repeated
  after 2, 6 and 20 seconds before the task fails. A task that failed, was stopped or was cut off keeps its whole
  conversation, so the next message ("go on") continues from where it was.
- **Only a question waits for the person.** See [Pace](#pace): a task that goes round in circles ends its request
  with a question, in the state `asking`.

The task runs in the service, so it goes on while the app shows another screen. It does not outlive the service:
the app stops `rigbrewd` when it quits, and the service stops by itself when the app is gone. A task that was
working then is `interrupted` after the next start and goes on with the next message.

`request_ms` is when the person sent the request that is being worked on.

### Undo

Before the first change of each request (a write, an edit or an allowed command) the folder is recorded, and
`POST .../undo` puts it back. A task lists these in `undo`, oldest first: `{"request", "kind", "undone", ...}`, where
`request` counts the person's messages from 1. Each call takes back the newest request that was not taken back yet
and has something to take back (outside git, a request in which only commands ran has nothing saved).

- **In a git repository** (`kind: "git"`, the folder has its own `.git` folder) the working tree is kept as a
  commit on the hidden ref `refs/rigbrew/<task>/<request>`, built with a temporary index file. The index, the
  branch and the stash are not touched. Undo writes every tracked and untracked file back as it was in that commit
  and removes the files made since, also when a command made them. What git ignores is left alone, and so is an
  untracked file over 50 MB. The state just before the undo is kept on `refs/rigbrew/<task>/<request>-undone`.
  Commits a command made stay on the branch. The refs go when the task is removed.
- **Anywhere else** (`kind: "copies"`) each file is copied to `agent/undo/<task>/<request>/` in the data folder
  before the task's own write or edit touches it. Undo puts those copies back and removes the files the task
  created. What a command changed cannot be taken back there; `commands: true` on the entry says one ran.

The task gets a `note` step that says what came back, and the model is told on its next turn.

- **Files stay inside the folder.** Every path is resolved through the file system first, so `..`, an absolute
  path and a symbolic link that leads out are refused alike. Nothing inside `.git` is written. Rigbrew's own data
  folder cannot be chosen, nor a folder that contains it.
- **Commands wait for a yes, and on macOS run in a sandbox.** `run_command` runs a shell command with the folder as
  its working directory, for two minutes at most. On macOS it runs under the system's sandbox: it can read what the
  person can, write only inside the folder and the temporary folders, and reach the network only when the task was
  given the web. `start_app` and `check_project` run their commands the same way; an app started with `start_app`
  may also listen on this computer's own address, and the task's commands may talk to that one port. The usual
  places programs are installed to are added to the command's `PATH`, and Go gets a build cache of its own in the
  temporary folder (its usual one cannot be written). Tools that insist on writing elsewhere in the home folder
  (Xcode's and Gradle's caches, a package manager's store while it installs) are refused there and say "Operation
  not permitted". On Windows and Linux a command is not confined yet and can do whatever the person's account can.
- **Looking opens this computer only.** `look_at_page` refuses every address that is not `localhost`, a name
  under it or a loopback address, in the service and again in the app; the hidden window it uses can call nothing
  of the app and cannot leave this computer. `simulator` runs fixed commands with checked arguments.
- **Everything is capped.** A tool result is at most about 4,000 characters and says when it was cut; a task stops
  after 40 model turns for one request and says so.
- **Web tools leave the machine.** `web_search` sends the query to Rigbrew's server (`GET /v1/search`), which asks a search service (Brave Search) and stores nothing; `fetch_url` gets one
  http(s) page (public addresses only, checked again at every redirect; 15 seconds, 2 MB) and returns its text.
  They are off unless the task says `"web": true`.

## Studio's assistant

A task with `"film": "<id>"` is a conversation about one film: the person says what they want and the model does
it on the film, one step at a time, in the same loop as Code (pace, rolling notes, the plan, streaming, stopping).
It reads and changes nothing but the film, through these tools:

| Tool | What it does |
|---|---|
| `read_film`, `read_shot {shot}` | The film in words (name, size, length, look, language, cast, every shot with its number, length, state, description and lines, the sound tracks), or one shot with everything about it. |
| `look_at_shot {shot, at?}` | A picture of a made shot (its middle, or `at` seconds into it) goes to the model with the result. Offered only when the model takes pictures. |
| `edit_shot {shot, description?, seconds?, lines?, add_line?, cast?, continues?}` | Changes one shot. `lines` replaces what is said (`[{who, text}]`, `who` a name from the cast or `narrator`), `add_line` adds one, `cast` names who is in it (a name the description lacks is added to it), `seconds` is snapped to a length the video model makes. |
| `add_shot {description, after?, seconds?, lines?}`, `remove_shot {shot}`, `move_shot {shot, to}` | A new shot (at the end, or after a shot; `0` for the start), one taken out (its clip stays), one moved to another place. |
| `set_look {look}`, `set_language {language}` | The look every shot shares; the language the lines are written in. |
| `cast_add {name, look}`, `cast_edit {name, look?, new_name?}` | Someone new in the cast; a look or a name changed (a new name is changed in the descriptions that name them). |
| `rewrite_shot {shot, hint}` | The chat model writes the description again, as `.../rewrite` does, and it goes in like any edit. |
| `subtitles {show?, burn?}`, `add_sound_region {file, at? or shot?, track?}` | Subtitles on or off and burned in; a sound file of the film's library laid on a sound track, as long as the file. |
| `make_shots {shots?}`, `dub {language?}`, `export` | Shots into the render queue (it does not wait for them), a dub (it waits for it), the export (it waits for it). |

Shots are named by their number (1 is the first); the film as it is now goes to the model on every turn. Every
change goes through the same save as `PUT /v1/studio/projects/{id}`, so it is checked the same way and a made shot
whose description changes is marked `changed`. A refusal is one plain sentence ("There is no shot 9. The shots are 1
to 5.", "Studio needs the video model. Add it under Models."). A change step carries its result's first line as
the line the person reads ("Shot 3: description changed") and `diff`: what it was and what it is, as lines that
start with `- ` and `+ `.

Edits go ahead in every mode but Read only, which offers no tool that changes the film. `make_shots`, `dub` and
`export` wait for the person (`pending.command` says what and about how long, like "Make 3 shots (1, 2 and 3): about
12 minutes") unless the mode is `bypass`; `always` stops the asking for that tool for the rest of the
conversation. `selection` (shot ids) is said to the model with the request ("Shots 2 and 3 are selected in the
tracks.") and kept as a `note` step. After a few turns that changed the film, the first `finish` is answered once
with the request, word for word. The app holds its own saves while a request is worked on, reads the film after
each change, keeps what the person edited meanwhile, and makes the whole request one step of undo ("What the
assistant did").

## Skills

A skill is a folder with a `SKILL.md`: YAML front matter with `name` and `description`, then instructions (the
open Agent Skills format). Skills are read from `skills/` in the data folder and from `.rigbrew/skills/` in the
task's folder; twelve come built in ([`daemon/skills`](../daemon/skills)). The model sees the names and
descriptions and reads a skill's instructions with `use_skill` when it needs them.

- **Where a skill is offered.** `metadata: where: code` in the front matter keeps a skill to Code tasks (it needs a
  folder), `where: chat` to the chat; without it the skill is offered in both. The chat is told about at most twelve
  skills, one line each; an agent with pinned skills is offered those and no others.
- **Other files.** A skill folder may hold more text files (`reference/x.md`, a template). `use_skill` lists them
  and `read_skill_file {name, path}` reads one: text only, capped like every tool result, and only from inside the
  skill's own folder (a path with `..`, an absolute path and a link that leads out are refused).
- **On and off.** `skills.json` in the data folder holds the names that are switched off. A skill that is off is
  not offered anywhere.
- **Your own.** `POST /v1/skills` and `PUT /v1/skills/{name}` write `skills/<name>/SKILL.md`: a name of 1 to 64 of
  `a-z`, `0-9` and `-`, a description of at most 300 characters, instructions of at most 20,000 (a model is handed
  the first 6,000). Changing a built-in skill saves the person's own version, which stands in for it until it is
  deleted.
- **From outside.** `POST /v1/skills/import` reads a folder on this computer, or a folder of a public repository on
  GitHub (`https://github.com/owner/repo`, `.../tree/<branch>/<path>`, or the address of a `SKILL.md`): one request
  to GitHub's API for the list of files and one to `raw.githubusercontent.com` for each file, without an account.
  The address may name one skill or a folder of skills. A skill is refused as a whole when anything in it is not
  text (by its name or by its content), when it has more than 40 files or more than 200 KB, or when a path would
  leave its folder. Nothing is saved until the names are sent back in `save`, and nothing of a skill is ever run.

## Connectors

A connector is a server that speaks the [Model Context Protocol](https://modelcontextprotocol.io): a program
started on this computer and talked to over its standard input and output, or a web address (streamable HTTP).
The service uses the official Rust SDK (`rmcp`) to say hello (`initialize`), list the tools (`tools/list`, every
page) and call one (`tools/call`).

```sh
# a program
curl -s -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name": "Notes", "kind": "stdio", "command": "npx", "args": ["-y", "@modelcontextprotocol/server-memory"], "env": {"MEMORY_FILE_PATH": "/Users/me/notes.jsonl"}}' \
  http://127.0.0.1:$PORT/v1/connectors
# a web address
curl -s -H "Authorization: Bearer $TOKEN" -H 'Content-Type: application/json' \
  -d '{"name": "Remote", "kind": "http", "url": "https://example.com/mcp", "token": "..."}' \
  http://127.0.0.1:$PORT/v1/connectors
```

- **Nothing runs by being saved.** A connector starts on `POST .../start`. One the person left running is started
  again when the service starts; all of them stop when it ends. A connector found in a project folder is never
  started: only what the person added here exists.
- **Secrets stay in.** `connectors.json` in the data folder is readable by the owner only. The values of `env` and
  the `token` are written there and handed to the program or the address, and nowhere else: the API answers with the
  names of the variables and `has_token`, a usage event carries the kind of connector and nothing more, and the last
  line a program wrote before it failed has the secrets taken out before it is shown. In a `PUT`, a variable with
  `null` keeps its saved value and a missing `token` keeps the saved token.
- **A web address is https.** Plain `http://` is accepted only for `localhost`, `127.0.0.1` and `[::1]`.
- **State.** `stopped`, `starting`, `running` or `failed`, with `error` (one line) for a failure, `tools` (how many)
  and `about` (what the server says it is, cut to one line). A program that ends by itself shows as `failed`.
- **A model never gets the list of tools.** The chat engine has room for 8192 tokens. A model that may use
  connectors is told one line for each running connector and gets two tools: `find_tool {query}` answers with the
  three best matches (name, one line of description, the arguments with their types) and `call_tool {connector,
  tool, arguments}` runs one.
- **The person is asked first.** In a Code task every `call_tool` waits for a yes that shows the connector, the
  tool and the arguments, unless the tool is in the connector's `always_allow` or was allowed for the task; in a
  read-only task it always waits. In the chat the question is in the conversation. `POST .../call` itself does not
  ask: it is what the app calls after the yes, and what "Try it" in the app uses.
- **What comes back is text from outside.** Names, descriptions and results are written by the connector. They reach
  a model only as the result of a tool call, marked as such, with control characters taken out and capped at about
  4,000 characters. Pictures and sound are named, not passed on.
- **Ready-made ones.** `gallery` in `GET /v1/connectors` lists a few servers from the Model Context Protocol
  project (files, Git, fetch, memory, SQLite, time). Each is only a filled-in form: `npx` ones need Node.js, `uvx`
  ones need Python with uv, and `programs` says whether those are installed.

## What leaves the machine

Models run locally and prompts are never sent to a model provider. `rigbrewd` sends usage data to
`https://api.rigbrew.dev` in two layers, both defined in [`core/src/events.rs`](../core/src/events.rs) and
[`daemon/src/cloud.rs`](../daemon/src/cloud.rs):

- **Basic usage, always.** Event kinds, counts, sizes, speeds, errors, hardware class, app version and a random
  install id. Before it is stored, every payload is reduced by `strip_content`: no text people typed, no model
  output, no strings longer than 120 characters, no account id. The server strips it again on arrival.
- **Content, only with consent and an account.** The text of prompts and answers. Builds from this repository have
  no account module, so they never send content.

A Code task with web search allowed sends its search queries to Rigbrew's server, which passes them to a search service (Brave Search) without storing them, and fetches the pages the model opens.
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
names must end in `.gguf`. Files larger than 200 GB, parts of split models (`-00001-of-00003.gguf`), importance
matrices (`imatrix`) and vision parts (`mmproj`) are not offered as models and are counted in `skipped`. Refused links
answer 400, repos that are private or do not exist 404, and a site that cannot be reached 502, each with a sentence in
`error.message`.

**Vision parts.** When the repo has `mmproj` files, the one that goes with a model file comes with it (`projector`):
the one in the same folder whose name shares the most with the model's, in 8-bit (`Q8`) before 16-bit (`F16`, `BF16`)
before 32-bit before anything with fewer bits, the smaller of two equal ones. It becomes the package's second file
(role `mmproj`), is downloaded and checked with the model, and the engine is started with it (`--mmproj`), so the
model takes pictures: those a Code task takes with `look_at_page` and `simulator`, and those the person sends. A
model added before Rigbrew fetched vision parts is `installed: false` in a new lookup when its repo has one; adding it
again keeps the model file (it is checked, not downloaded) and fetches the vision part. `GET /v1/packages` says per
version whether it has one (`vision`).

A file needs its size and about 15% more in memory, its vision part included (`needs_mb`). `fits` compares that with `budget_mb`, the
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

### The community catalog

Rigbrew suggests a pool of community models. The pool is one file, so it grows without an app release:

- `https://models.rigbrew.dev/community.json`, served as `application/json`;
- `https://models.rigbrew.dev/community.json.minisig`, its minisign signature, made with the key that signs the
  manifest (`registry/rigbrew-manifest.pub`): `minisign -S -s <secret key> -m community.json`.

```json
{
  "version": 1,
  "updated": "2026-11-25",
  "models": [
    {
      "id": "example-chat",
      "name": "Example Chat",
      "kind": "chat",
      "by": "Example Lab",
      "url": "https://huggingface.co/example-owner/Example-Chat-GGUF",
      "license": "Apache-2.0",
      "about": "Coding and reasoning; about 20 GB of memory.",
      "status": "add",
      "note": "Non-commercial licence",
      "more": [{"label": "Original", "url": "https://huggingface.co/example-lab/Example-Chat"}]
    }
  ]
}
```

| Field | Meaning |
|---|---|
| `id` | Names the entry; the first entry with an id keeps it. Up to 80 characters. |
| `name`, `by`, `license` | Shown as they are. Up to 80, 80 and 60 characters. |
| `kind` | `chat`, `image`, `video`, `speech` (text to speech), `listen` (speech to text), `music` or `tool`. The app lists any other kind under "Other". |
| `url` | The model's page: an `https://huggingface.co/...` or `https://github.com/...` link, with no port or sign-in in it. An entry with any other link is left out. |
| `about` | One line, up to 200 characters. |
| `status` | `add`: a chat model Rigbrew can run now; the app puts `url` through `POST /v1/community/lookup` and the person picks a file. `listed`: a model to read about that Rigbrew cannot run yet. Anything else, and `add` on an entry that is not `chat` or whose link the lookup does not accept, counts as `listed`. |
| `note` | Optional, up to 120 characters ("Non-commercial licence"). |
| `more` | Optional, up to 4 links `{label, url}` under the same rule as `url`; others are dropped. |

An entry without an `id`, a `name` or an accepted `url` is left out; the rest of the file is still read. Text is cut
to the lengths above and loses its control characters. The file may be 512 KB at most and the first 500 entries
count. A file that is larger, is not JSON or has no `models` list is refused as a whole.

`GET /v1/community/catalog` fetches the file and its signature, checks the signature, and answers with the entries
after these checks; every `add` entry also says whether a file of that repo is already among the person's community
models (`added`). A file whose signature checks out is kept in `state/community-catalog.json` (with its `.minisig`)
in the data folder; the kept copy is checked again each time it is read. The service asks for the file again after 6
hours, 10 minutes after a fetch that failed, or at once with `?refresh=1`. When the file cannot be fetched or its
signature does not check out, the answer is the kept copy with `"stale": true`, and with nothing kept
`{"updated": "", "stale": true, "models": []}`; the endpoint never answers with an error. Looking at the catalog
records no event.

## Environment

| Variable | Effect |
|---|---|
| `RIGBREW_DATA_DIR` | Use another data folder (packages, chats, the event queue). |
| `RIGBREW_MANIFEST` | Manifest file or URL, instead of the default. |
| `RIGBREW_COMMUNITY_URL` | Community catalog file or URL, instead of `https://models.rigbrew.dev/community.json`. Its signature is read from the same place with `.minisig` added and is checked against the same key. A file without a signature is accepted only by a development build or with `RIGBREW_ALLOW_UNSIGNED=1`, like the manifest; a URL always needs its signature. |
| `RIGBREW_BASE_URL` | Download package files from another mirror (files are still checked against the signed manifest). |
| `RIGBREW_ENGINE_DIR` | Use engine programs from this folder instead of downloading them. |
| `RIGBREW_PORT` | Listen on a fixed port. |
| `RIGBREW_IDLE_MINUTES` | Take models out of memory after this many minutes without use (10; 0 keeps them). |
| `DO_NOT_TRACK`, `RIGBREW_NO_TELEMETRY` | Send nothing. |
