// Talks to rigbrewd, the local service, directly over HTTP on 127.0.0.1.
// Inside the app the port and token come from the native side, which also starts the service
// again when it is gone. In a plain browser (pnpm dev) they come from the dev server, which
// reads daemon.json, or from VITE_RIGBREW_PORT and VITE_RIGBREW_TOKEN.
//
// The port and token change every time the service starts. When a request cannot reach the
// service, this module forgets the connection, tells the app (`onLink`), and keeps asking for
// a new one until the service answers again.

import type { Agent } from './agents';
import type { Activity, AppConfig, CommunityFound, Consent, Job, Package, QueuedEvent, Session, System, Usage } from './types';

export const isTauri = typeof window !== 'undefined' && '__TAURI_INTERNALS__' in window;

interface Connection {
	base: string;
	token: string;
}

let connection: Promise<Connection> | null = null;

async function connect(): Promise<Connection> {
	if (isTauri) {
		const { invoke } = await import('@tauri-apps/api/core');
		const info = await invoke<{ port: number; token: string }>('daemon_info');
		return { base: `http://127.0.0.1:${info.port}`, token: info.token };
	}
	try {
		const answer = await fetch('/__daemon', { cache: 'no-store' });
		if (answer.ok) {
			const info = (await answer.json()) as { port?: number; token?: string };
			if (info.port && info.token) return { base: `http://127.0.0.1:${info.port}`, token: info.token };
		}
	} catch {
		// not the dev server; fall through to the environment
	}
	const port = import.meta.env.VITE_RIGBREW_PORT;
	const token = import.meta.env.VITE_RIGBREW_TOKEN;
	if (!port || !token) {
		throw new Error('No running service found. Start rigbrewd, or set VITE_RIGBREW_PORT and VITE_RIGBREW_TOKEN.');
	}
	return { base: `http://127.0.0.1:${port}`, token };
}

export class ApiError extends Error {
	constructor(
		message: string,
		/** HTTP status, or 0 when the service could not be reached at all. */
		public status = 0
	) {
		super(message);
	}
}

/** Said when a request was cut off because the service went away. */
export const LOST = 'The local service stopped answering. Rigbrew is bringing it back; try again in a moment.';

// --- the link to the service ---------------------------------------------------------------

let linkDown = false;
let probing = false;
let linkListener: (up: boolean) => void = () => {};

/** Called with `false` when the service stops answering and with `true` when it is back. */
export function onLink(listener: (up: boolean) => void) {
	linkListener = listener;
}

const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));

/** The service did not answer: forget where it was and start looking for it again. */
function lost() {
	connection = null;
	if (!linkDown) {
		linkDown = true;
		linkListener(false);
	}
	probe();
}

async function probe() {
	if (probing) return;
	probing = true;
	for (let wait = 600; ; wait = Math.min(wait * 1.5, 4000)) {
		await pause(wait);
		try {
			// In the app this call also starts the service again when it is gone.
			const conn = await connect();
			const answer = await fetch(conn.base + '/v1/consent', { headers: { Authorization: `Bearer ${conn.token}` } });
			if (answer.ok) {
				connection = Promise.resolve(conn);
				break;
			}
		} catch {
			// still gone
		}
	}
	probing = false;
	linkDown = false;
	linkListener(true);
}

/** Tells this module that a stream it handed out broke in the middle (the service went away). */
export function reportLost() {
	lost();
}

// --- requests --------------------------------------------------------------------------------

async function send(method: string, path: string, body?: unknown, signal?: AbortSignal, retried = false): Promise<Response> {
	const mine = (connection ??= connect());
	let conn: Connection;
	try {
		conn = await mine;
	} catch (e) {
		if (connection === mine) connection = null;
		lost();
		throw new ApiError(e instanceof Error ? e.message : String(e));
	}
	let response: Response;
	try {
		response = await fetch(conn.base + path, {
			method,
			signal,
			headers: {
				Authorization: `Bearer ${conn.token}`,
				...(body === undefined ? {} : { 'Content-Type': 'application/json' })
			},
			body: body === undefined ? undefined : JSON.stringify(body)
		});
	} catch (e) {
		if (e instanceof DOMException && e.name === 'AbortError') throw e;
		if (connection === mine) lost();
		throw new ApiError(LOST);
	}
	if (response.status === 401 && !retried) {
		// The token belongs to a service that has been replaced. Ask where the new one is, once.
		if (connection === mine) connection = null;
		return send(method, path, body, signal, true);
	}
	if (!response.ok) {
		let message = `The local service answered ${response.status}.`;
		try {
			const data = await response.json();
			message = data?.error?.message ?? message;
		} catch {
			// not JSON; keep the status message
		}
		throw new ApiError(message, response.status);
	}
	return response;
}

/** A multipart upload (a recording), with the same connection, token and errors as `send`. */
async function sendForm(path: string, form: FormData): Promise<Response> {
	const conn = await (connection ??= connect()).catch((e) => {
		connection = null;
		lost();
		throw new ApiError(e instanceof Error ? e.message : String(e));
	});
	let response: Response;
	try {
		response = await fetch(conn.base + path, { method: 'POST', headers: { Authorization: `Bearer ${conn.token}` }, body: form });
	} catch {
		lost();
		throw new ApiError(LOST);
	}
	if (!response.ok) {
		let message = `The local service answered ${response.status}.`;
		try {
			message = (await response.json())?.error?.message ?? message;
		} catch {
			// not JSON
		}
		throw new ApiError(message, response.status);
	}
	return response;
}

async function json<T>(method: string, path: string, body?: unknown): Promise<T> {
	return (await send(method, path, body)).json() as Promise<T>;
}

/** For endpoints an older service does not have: `null` instead of an error. */
async function optional<T>(method: string, path: string, body?: unknown): Promise<T | null> {
	try {
		return await json<T>(method, path, body);
	} catch (e) {
		if (e instanceof ApiError && (e.status === 404 || e.status === 405)) return null;
		throw e;
	}
}

const enc = encodeURIComponent;

export const system = () => json<System>('GET', '/v1/system');
export const packages = async () => (await json<{ packages: Package[] }>('GET', '/v1/packages')).packages;
export const pull = async (id: string, variant?: string, acceptTerms = false) =>
	(await json<{ job: string }>('POST', `/v1/packages/${enc(id)}/pull`, { ...(variant ? { variant } : {}), ...(acceptTerms ? { accept_terms: true } : {}) })).job;
/** Removes one installed build, or every build of the package. Files another build still needs stay. */
export const removePackage = (id: string, variant?: string) =>
	json<{ removed: string[]; freed_bytes: number }>('DELETE', `/v1/packages/${enc(id)}${variant ? `?variant=${enc(variant)}` : ''}`);

/** Asks the service what a link to a model on Hugging Face or GitHub holds. */
export const communityLookup = (url: string) => json<CommunityFound>('POST', '/v1/community/lookup', { url });
/** Adds one file of a looked-up repo as a community chat model and starts its download. */
export const communityAdd = (repo: string, file: string) => json<{ job: string; id: string; name: string }>('POST', '/v1/community', { repo, file });
/** Removes a community model: its files and its place in the list. */
export const communityRemove = (id: string) => json<{ removed: string; freed_bytes: number }>('DELETE', `/v1/community/${enc(id)}`);

export const job = (id: string) => json<Job>('GET', `/v1/jobs/${enc(id)}`);
/** Every download of this run of the service, running or ended. */
export const jobs = async () => (await json<{ jobs: { id: string; job: Job }[] }>('GET', '/v1/jobs')).jobs;
/** Stops a running download. The partial file stays, so installing again resumes. */
export const cancelJob = (id: string) => json<{ cancelled: string }>('DELETE', `/v1/jobs/${enc(id)}`);

/** The accepted consent (or null) and the version of the text the service expects. */
export const consent = () => json<{ consent: Consent | null; current_version?: string }>('GET', '/v1/consent');
export const acceptConsent = async (version: string) => (await json<{ consent: Consent }>('POST', '/v1/consent', { version })).consent;
export const withdrawConsent = () => json<{ consent: null }>('DELETE', '/v1/consent');
/** Events waiting to be sent, newest first. `count` is the whole queue. */
export const pendingEvents = (limit = 200) => json<{ count: number; events: QueuedEvent[] }>('GET', `/v1/events/pending?order=newest&limit=${limit}`);
export const recordEvent = (kind: string, payload: Record<string, unknown> = {}) => json<{ recorded: boolean }>('POST', '/v1/events', { kind, payload });

/** Hands the service the signed-in account and its current ID token. False when the service is too old. */
export const putSession = async (session: Session) => (await optional('PUT', '/v1/session', session)) !== null;
export const deleteSession = async () => (await optional('DELETE', '/v1/session')) !== null;

/** Totals, disk use and sync status, computed by the service. Null when the service is too old. */
export const usage = () => optional<Usage>('GET', '/v1/usage');
/** Per-day activity, most used packages and totals, for the profile. Null when the service is too old. */
export const activity = (days = 365) => optional<Activity>('GET', `/v1/activity?days=${days}`);
/** Tries to send the queued events now. */
export const syncNow = () => optional<{ sent: number; pending: number; error?: string }>('POST', '/v1/sync');
/** The server's app config through the service (kept on disk there, so it also answers offline). */
export const appConfig = (version: string) => optional<AppConfig>('GET', `/v1/app/config?version=${enc(version)}`);

/** Starts a streamed chat completion. The caller reads the SSE body. */
export const chatStream = (body: unknown, signal: AbortSignal) => send('POST', '/v1/chat/completions', body, signal);

export interface LoadedEngine {
	package: string;
	variant: string;
	engine: string;
}

/**
 * Which model the service holds in memory, or null. While the service is loading a model
 * this call waits for the load to end, so its answer also marks the end of an engine swap.
 */
export const engine = async (signal?: AbortSignal) =>
	((await send('GET', '/v1/engine', undefined, signal)).json() as Promise<{ loaded: LoadedEngine | null }>).then((r) => r.loaded);
/** Every model in memory (a chat model and a picture or video model fit together on big machines). */
export const engines = async () =>
	((await send('GET', '/v1/engine')).json() as Promise<{ loaded: LoadedEngine | null; all?: LoadedEngine[] }>).then((r) => r.all ?? (r.loaded ? [r.loaded] : []));

/** Unloads the model and frees its memory. Returns the package that was loaded, if any. */
export const unloadEngine = async () => (await json<{ unloaded: string | null }>('DELETE', '/v1/engine')).unloaded;

export interface ImageRequest {
	model: string;
	prompt: string;
	size: string;
	steps: number;
	seed: number;
}

export interface ImageResponse {
	data: { b64_json: string }[];
	seconds: number;
	variant: string;
	/** The PNG as the service saved it. Missing on an older service. */
	image?: { id: string; path: string; bytes: number };
}

/** Asks the installed image package for one picture. Slow when the engine has to be swapped in. */
export const generateImage = async (body: ImageRequest, signal: AbortSignal) =>
	(await send('POST', '/v1/images/generations', body, signal)).json() as Promise<ImageResponse>;

/** Stage and steps of the picture being made, or null (also null from an older service). */
export const imageProgress = async () =>
	(await optional<{ progress: { stage: string; completed: number | null; total: number | null; elapsed_ms: number } | null }>('GET', '/v1/images/progress'))?.progress ?? null;

/** A saved picture. It needs the token, so it is fetched and shown through an object URL. */
export const imageFile = async (id: string) => (await send('GET', `/v1/images/files/${enc(id)}.png`)).blob();
/** Removes a saved picture. A picture that is already gone is not an error. */
export const deleteImageFile = async (id: string) => {
	await optional('DELETE', `/v1/images/files/${enc(id)}.png`);
};

// --- reading aloud ------------------------------------------------------------------------------

export interface Reading {
	/** The WAV, to play through an object URL (it needs the token, like pictures). */
	audio: Blob;
	/** Where the service saved it: `/v1/audio/files/<id>.wav`. */
	path: string | null;
	audioSeconds: number;
	seconds: number;
}

/** Reads text aloud with an installed voice package. Markdown is cleaned up by the service. */
export async function speak(model: string, input: string, signal?: AbortSignal): Promise<Reading> {
	const response = await send('POST', '/v1/audio/speech', { model, input }, signal);
	return {
		audio: await response.blob(),
		path: response.headers.get('x-rigbrew-audio'),
		audioSeconds: Number(response.headers.get('x-rigbrew-audio-seconds') ?? 0),
		seconds: Number(response.headers.get('x-rigbrew-seconds') ?? 0)
	};
}

// --- listening ---------------------------------------------------------------------------------

/** Writes down a recording with the installed listening package. `language`: a code such as "tr"; none lets the model tell. */
export async function transcribe(recording: Blob, language?: string) {
	const form = new FormData();
	form.append('file', recording, 'recording.wav');
	if (language) form.append('language', language);
	return (await sendForm('/v1/audio/transcriptions', form)).json() as Promise<{ text: string; language: string | null; duration: number; seconds: number }>;
}

// --- video ---------------------------------------------------------------------------------------

export interface VideoJobState {
	package: string;
	variant: string;
	state: 'loading' | 'running' | 'done' | 'failed' | 'cancelled';
	/** The engine's stage: conditioning, sampling, vae_decode, encoding, ... */
	stage: string;
	step: number | null;
	steps: number | null;
	elapsed_ms: number;
	width: number;
	height: number;
	frames: number;
	video: { id: string; path: string; bytes: number; mime: string } | null;
	error: string | null;
}

/** Starts a clip in the background; follow it with `videoJob`. */
export const startVideo = async (body: { model: string; prompt: string; size: string; frames: number; steps: number; seed: number }) =>
	(await json<{ job: string }>('POST', '/v1/videos/generations', body)).job;
export const videoJob = (id: string) => json<VideoJobState>('GET', `/v1/videos/jobs/${enc(id)}`);
/** Asks the service to stop a clip; it stops at the engine's next step. */
export const stopVideo = async (id: string) => {
	await optional('DELETE', `/v1/videos/jobs/${enc(id)}`);
};
/** A saved clip (`<id>.webm`), fetched with the token and shown through an object URL. */
export const videoFile = async (name: string) => (await send('GET', `/v1/videos/files/${enc(name)}`)).blob();

/** Saves a clip where the person chooses (app) or as a download (browser preview). */
export async function saveVideo(name: string, clip: Blob): Promise<'saved' | 'downloaded' | 'cancelled'> {
	if (isTauri) {
		const bytes = new Uint8Array(await clip.arrayBuffer());
		let binary = '';
		for (let i = 0; i < bytes.length; i += 0x8000) binary += String.fromCharCode(...bytes.subarray(i, i + 0x8000));
		const { invoke } = await import('@tauri-apps/api/core');
		const path = await invoke<string | null>('save_video', { name, data: btoa(binary) });
		return path ? 'saved' : 'cancelled';
	}
	const url = URL.createObjectURL(clip);
	download(name, url);
	setTimeout(() => URL.revokeObjectURL(url), 10_000);
	return 'downloaded';
}

// --- saved chats -------------------------------------------------------------------------------

/** One saved chat in the list, newest first. */
export interface ChatRow {
	id: string;
	title: string;
	created_ms: number;
	updated_ms: number;
	size: number;
}

/** The saved chats, or null when the service cannot save chats (an older service). */
export const chats = async () => (await optional<{ chats: ChatRow[] }>('GET', '/v1/chats'))?.chats ?? null;
/** One chat with the app's own data; null when it is not there. */
export const chatById = (id: string) => optional<ChatRow & { data: unknown }>('GET', `/v1/chats/${enc(id)}`);
/** Saves a chat (creates or replaces it). */
export const saveChat = (id: string, title: string, data: unknown) => json<ChatRow>('PUT', `/v1/chats/${enc(id)}`, { title, data });
export const deleteChat = (id: string) => optional('DELETE', `/v1/chats/${enc(id)}`);
export const deleteAllChats = () => optional('DELETE', '/v1/chats');

// --- agents and artifacts ------------------------------------------------------------------------

/** The agents the person made, or null when the service cannot keep them (an older service). */
export const agents = async () => (await optional<{ agents: Agent[] }>('GET', '/v1/agents'))?.agents ?? null;
/** Saves an agent (creates or replaces it) and returns it as stored. */
export const saveAgent = (agent: Agent) =>
	json<Agent>('PUT', `/v1/agents/${enc(agent.id)}`, {
		name: agent.name,
		instruction: agent.instruction,
		tools: agent.tools,
		starter: agent.starter,
		connectors: agent.connectors ?? false,
		skills: agent.skills ?? []
	});
export const deleteAgent = (id: string) => optional('DELETE', `/v1/agents/${enc(id)}`);

const frames = new Map<string, boolean>();

/**
 * The address of the page a chat artifact is shown in: an empty page from the service that
 * allows inline styles and scripts and nothing from anywhere else. The app's own window
 * forbids inline scripts, and a frame made from text inherits that, so a page the model wrote
 * goes into this one. Null when the service does not have it (an older service).
 */
export async function artifactFrame(): Promise<string | null> {
	let base: string;
	try {
		base = (await (connection ??= connect())).base;
	} catch {
		return null;
	}
	const url = `${base}/v1/artifacts/frame`;
	if (!frames.has(base)) {
		try {
			frames.set(base, (await fetch(url, { cache: 'no-store' })).ok);
		} catch {
			return null;
		}
	}
	return frames.get(base) ? url : null;
}

/** What happened lately, in words, for the profile. Null when the service is too old. */
export const recentActivity = async (limit = 50) =>
	(await optional<{ items: { ts_ms: number; kind: string; title: string; detail: string; package: string | null }[] }>('GET', `/v1/activity/recent?limit=${limit}`))?.items ?? null;

// --- things the system does for the app ------------------------------------------------------

function download(name: string, href: string) {
	const link = document.createElement('a');
	link.href = href;
	link.download = name;
	document.body.append(link);
	link.click();
	link.remove();
}

/**
 * Saves a PNG the person asked to keep. In the app a native save dialog picks the place; in a
 * plain browser the file goes to the downloads folder. Returns how it went.
 */
export async function saveImage(name: string, base64: string): Promise<'saved' | 'downloaded' | 'cancelled'> {
	if (!base64) throw new Error('The picture is not available any more.');
	if (isTauri) {
		const { invoke } = await import('@tauri-apps/api/core');
		const path = await invoke<string | null>('save_image', { name, data: base64 });
		return path ? 'saved' : 'cancelled';
	}
	download(name, `data:image/png;base64,${base64}`);
	return 'downloaded';
}

/** Saves a text the person asked to keep (the account export, an artifact), the same way. */
export async function saveText(name: string, text: string, type = 'application/json'): Promise<'saved' | 'downloaded' | 'cancelled'> {
	if (isTauri) {
		const { invoke } = await import('@tauri-apps/api/core');
		const path = await invoke<string | null>('save_text', { name, text });
		return path ? 'saved' : 'cancelled';
	}
	const url = URL.createObjectURL(new Blob([text], { type }));
	download(name, url);
	setTimeout(() => URL.revokeObjectURL(url), 10_000);
	return 'downloaded';
}

export async function copyText(text: string) {
	try {
		await navigator.clipboard.writeText(text);
	} catch {
		// Older webviews: fall back to a hidden field and the copy command.
		const field = document.createElement('textarea');
		field.value = text;
		field.style.cssText = 'position:fixed;opacity:0';
		document.body.append(field);
		field.select();
		document.execCommand('copy');
		field.remove();
	}
}

/** Opens a link in the system browser. */
export async function openExternal(url: string) {
	if (!/^https?:\/\//.test(url)) return;
	if (isTauri) {
		const { openUrl } = await import('@tauri-apps/plugin-opener');
		await openUrl(url);
	} else {
		window.open(url, '_blank', 'noopener');
	}
}

// --- Studio: films made of shots, kept in a folder ----------------------------------------------

/** One clip of a shot. Making a shot again keeps the clip before as a take. */
export interface StudioTake {
	n: number;
	/** The seed it was made with: the same seed and description give the same clip again. */
	seed?: number;
	seconds?: number;
	frames: number;
	made_frames?: number;
}

export interface StudioShot {
	id: string;
	prompt: string;
	frames: number;
	/** empty, queued, making, done, failed, or changed (made, then the description changed). */
	state: 'empty' | 'queued' | 'making' | 'done' | 'failed' | 'changed';
	error?: string;
	seconds?: number;
	/** The frames its clip really has: the video model rounds a length to what it can make. */
	made_frames?: number;
	/** Seconds cut from the start and the end when the film is played or exported. */
	trim_start?: number;
	trim_end?: number;
	/** Starts from the last picture of the shot before it. */
	continues?: boolean;
	/** How the film goes from the shot before into this one. Not used when the shot continues. */
	join?: 'cut' | 'dissolve' | 'black' | '';
	/** Seconds that join lasts, 0.25 to 2. Half a second unless set. */
	join_seconds?: number;
	/** How loud the shot's own sound is, 0 to 1.5. */
	volume?: number;
	mute?: boolean;
	/** The clips made for this shot (the last four), and the number of the one in use. */
	takes?: StudioTake[];
	take?: number;
	/** A seed to make the next clip with; without one every clip gets a new seed. */
	seed?: number | null;
}

/** A stretch of a sound file at a place in the film. */
export interface StudioRegion {
	id: string;
	/** The file by its full path, or by its name inside the film's folder. */
	file: string;
	/** Seconds into the film where it starts. */
	start: number;
	/** Seconds into the file where it starts, and how long it plays from there. */
	offset?: number;
	length: number;
	/** 1 as the file is, up to 2. */
	gain?: number;
	fade_in?: number;
	fade_out?: number;
	/** How many times the stretch plays, one after another. */
	loops?: number;
}

/** A sound track: music, narration or effects. */
export interface StudioTrack {
	id: string;
	name: string;
	color: string;
	/** 0 to 2. */
	volume?: number;
	/** -1 is all left, 1 all right. */
	pan?: number;
	mute?: boolean;
	solo?: boolean;
	regions: StudioRegion[];
}

/** The channel strip of the shots' own sound as a whole. */
export interface StudioStrip {
	volume: number;
	pan: number;
	mute: boolean;
	solo: boolean;
}

export interface StudioProject {
	id: string;
	name: string;
	folder: string;
	size: string;
	shots: StudioShot[];
	tracks?: StudioTrack[];
	shot_track?: StudioStrip;
	/** How loud the whole film is: 1 as mixed, up to 2. */
	master?: number;
	/** The film is exported by itself when its last shot is made. */
	auto_export?: boolean;
	/** From before sound tracks: the service turns it into a track when the film is read. */
	sound?: string;
	sound_volume?: number;
	/** The sound file plays once (a narration) and does not repeat. */
	sound_once?: boolean;
	/** The film comes up from black at its start, and goes down to black at its end. */
	fade_in?: boolean;
	fade_out?: boolean;
	/** How the film is written to its MP4. */
	export?: StudioExportOptions;
	updated_ms: number;
}

export interface StudioView {
	project: StudioProject;
	progress: { shot: string; stage: string; step: number | null; steps: number | null; elapsed_ms: number } | null;
	rendering: boolean;
	made: number;
	seconds: number;
	/** Whether the installed video model can start a shot from a picture. */
	can_continue?: boolean;
	/** Where the export of this film is. */
	export?: StudioExport;
	/** About how many seconds of work are left: `all` for every shot that is not made, `left` for what is waiting or being made. */
	estimate?: { all: number; left: number };
}

/** What a shot of `frames` frames takes on this machine, in seconds: plain, and continuing the shot before. */
export interface StudioPace {
	frames: number;
	plain: number;
	continuing: number;
	/** False when no shot has been made here yet and the numbers are the usual ones. */
	measured: boolean;
}

export interface StudioListing {
	id: string;
	name: string;
	folder: string;
	shots?: number;
	made?: number;
	missing?: boolean;
}

/** The films, and `home`: the folder new films go into when none is chosen. */
export const studioList = () => json<{ projects: StudioListing[]; home?: string | null; pace?: StudioPace; can_continue?: boolean }>('GET', '/v1/studio/projects');
/** Opens the film in `folder`, or starts one there. Without a folder the service makes one named after the film. */
export const studioOpen = (folder: string, name = '') => json<StudioView>('POST', '/v1/studio/projects', { folder, name });
export const studioRead = (id: string) => json<StudioView>('GET', `/v1/studio/projects/${enc(id)}`);
/** `scale` 2 enlarges the picture to twice its width and height; `quality` is small, balanced or best; `name` is the file's name without its ending. */
export interface StudioExportOptions {
	scale?: number;
	quality?: string;
	name?: string;
}

export interface StudioSave {
	name?: string;
	size?: string;
	auto_export?: boolean;
	sound?: string;
	sound_volume?: number;
	sound_once?: boolean;
	fade_in?: boolean;
	fade_out?: boolean;
	export?: StudioExportOptions;
	master?: number;
	shot_track?: StudioStrip;
	tracks?: StudioTrack[];
	shots?: {
		id?: string;
		prompt: string;
		frames: number;
		trim_start?: number;
		trim_end?: number;
		continues?: boolean;
		join?: string;
		join_seconds?: number;
		volume?: number;
		mute?: boolean;
		seed?: number | null;
	}[];
}
export const studioSave = (id: string, body: StudioSave) => json<StudioView>('PUT', `/v1/studio/projects/${enc(id)}`, body);
/** The voice reads a text; the reading is saved in the film's folder and set on a narration track at `at` seconds. */
export const studioNarration = (id: string, text: string, at = 0, track?: string) =>
	json<StudioView & { region: { track: string; id: string; seconds: number } }>('POST', `/v1/studio/projects/${enc(id)}/narration`, { text, at, ...(track ? { track } : {}) });
/** The chat model writes the description of one shot again. Nothing is saved. */
export const studioRewrite = async (id: string, shot: string, prompt: string, hint = '') =>
	(await json<{ prompt: string }>('POST', `/v1/studio/projects/${enc(id)}/shots/${enc(shot)}/rewrite`, { prompt, hint })).prompt;

/** A sound file in a film's library. */
export interface StudioSound {
	file: string;
	name: string;
	bytes: number;
	/** In the film's folder, added from elsewhere, or only used by a region. */
	from: 'folder' | 'added' | 'used';
	missing?: boolean;
}
export const studioSounds = async (id: string) => (await json<{ sounds: StudioSound[] }>('GET', `/v1/studio/projects/${enc(id)}/sounds`)).sounds;
/** Adds a sound file by its full path, or every sound file directly in a folder. */
export const studioAddSound = (id: string, path: string) => json<{ sounds: StudioSound[]; added: string[] }>('POST', `/v1/studio/projects/${enc(id)}/sounds`, { path });
export const studioRemoveSound = async (id: string, file: string) =>
	(await json<{ sounds: StudioSound[] }>('DELETE', `/v1/studio/projects/${enc(id)}/sounds?file=${enc(file)}`)).sounds;
/** The lowest and highest value of every fiftieth of a second of a sound, each from -127 to 127. */
export interface StudioPeaks {
	rate: number;
	seconds: number;
	peaks: number[];
}
export const studioPeaks = (id: string, of: { shot: string } | { file: string }) =>
	json<StudioPeaks>('GET', `/v1/studio/projects/${enc(id)}/peaks?${'shot' in of ? `shot=${enc(of.shot)}` : `file=${enc(of.file)}`}`);
/** A sound file of the film, to play in the app. */
export const studioSoundFile = async (id: string, file: string) => (await send('GET', `/v1/studio/projects/${enc(id)}/sound?file=${enc(file)}`)).arrayBuffer();
export const studioForget = (id: string) => json<unknown>('DELETE', `/v1/studio/projects/${enc(id)}`);
export const studioRender = (id: string, shots?: string[]) => json<StudioView>('POST', `/v1/studio/projects/${enc(id)}/render`, shots ? { shots } : {});
export const studioStop = (id: string) => json<StudioView>('DELETE', `/v1/studio/projects/${enc(id)}/render`);
export const studioStoryboard = async (id: string, idea: string, shots: number) =>
	(await json<{ shots: { prompt: string; continues?: boolean }[] }>('POST', `/v1/studio/projects/${enc(id)}/storyboard`, { idea, shots })).shots;
export interface StudioExport {
	state: 'none' | 'working' | 'done' | 'failed';
	file?: string;
	error?: string;
	shots?: number;
	bytes?: number;
	/** Seconds of film in the file. */
	seconds?: number;
}
/** Shows the exported file in the system's file manager. */
export const studioReveal = (id: string) => json<{ shown: string }>('POST', `/v1/studio/projects/${enc(id)}/reveal`);
export const studioExport = (id: string) => json<StudioExport>('POST', `/v1/studio/projects/${enc(id)}/export`, {});
export const studioExportState = (id: string) => json<StudioExport>('GET', `/v1/studio/projects/${enc(id)}/export`);
export const studioClip = async (id: string, shot: string, take?: number) =>
	(await send('GET', `/v1/studio/projects/${enc(id)}/clips/${enc(shot)}${take ? `.${take}` : ''}.webm`)).blob();
/** One small picture of a made shot: of the take in use, or of the take named. */
export const studioPoster = async (id: string, shot: string, take?: number) =>
	(await send('GET', `/v1/studio/projects/${enc(id)}/clips/${enc(shot)}${take ? `.${take}` : ''}.jpg`)).blob();
export const studioUseTake = (id: string, shot: string, take: number) => json<StudioView>('POST', `/v1/studio/projects/${enc(id)}/shots/${enc(shot)}/take`, { take });
export const studioDeleteTake = (id: string, shot: string, take: number) => json<StudioView>('DELETE', `/v1/studio/projects/${enc(id)}/shots/${enc(shot)}/takes/${take}`);

// --- Code: tasks a model works on in a folder (the loop runs in the service) ------------------

export type AgentMode = 'ask' | 'read_only' | 'auto_edits' | 'bypass';
/** `waiting`: a step waits for a yes or no. `asking`: the task put a question to the person and goes on with their next message. */
export type AgentState = 'running' | 'waiting' | 'asking' | 'done' | 'failed' | 'stopped' | 'interrupted';

export interface AgentToolStep {
	kind: 'tool';
	call: string;
	name: string;
	arguments: Record<string, unknown>;
	status: 'running' | 'waiting' | 'done' | 'failed' | 'denied' | 'stopped';
	result: string | null;
	diff: string | null;
}

export type AgentStep = { kind: 'user' | 'model' | 'note' | 'question'; text: string } | AgentToolStep;

/** A step that waits for the person: a change to a file (with its diff), a command, or a connector's tool. */
export interface AgentPending {
	call: string;
	tool: string;
	/** The file, or for `call_tool` the name of the connector. */
	path: string | null;
	command: string | null;
	diff: string | null;
}

export interface AgentTaskRow {
	id: string;
	folder: string;
	title: string;
	model: string;
	mode: AgentMode;
	state: AgentState;
	created_ms: number;
	updated_ms: number;
	/** When the person sent the request that is being worked on. Missing from an older service. */
	request_ms?: number;
}

/** A request that can be taken back. `request` counts the person's messages from 1. */
export interface AgentUndo {
	request: number;
	kind: 'git' | 'copies';
	/** Outside git: the files that were saved before the task touched them. */
	files: { path: string; existed: boolean }[];
	/** Outside git: a command ran, and what it changed cannot be taken back. */
	commands: boolean;
	undone: boolean;
}

/** One step of the plan the model keeps while it works. */
export interface AgentPlanStep {
	text: string;
	status: 'pending' | 'doing' | 'done';
}

export interface AgentTask extends AgentTaskRow {
	web: boolean;
	/** "Use connectors", and the connector tools allowed for the rest of the task. Missing from an older service. */
	mcp?: { on: boolean; always: string[] };
	steps: AgentStep[];
	pending: AgentPending | null;
	error: string | null;
	/** Missing from a service that is older than the plan. */
	plan?: AgentPlanStep[];
	/** The folder before each request that changed it, oldest first. */
	undo?: AgentUndo[];
	/** The answer the model is writing right now, as far as it got. Empty between turns. */
	writing?: string;
}

export interface AgentSkill {
	name: string;
	description: string;
	source: 'built-in' | 'yours' | 'folder';
}

const taskOf = async (answer: Promise<{ task: AgentTask }>) => (await answer).task;

/** `null` from a service that is too old to have Code. */
export const agentTasks = async (folder = '') => (await optional<{ tasks: AgentTaskRow[] }>('GET', `/v1/agent/tasks?folder=${enc(folder)}`))?.tasks ?? null;
export const agentTask = (id: string) => taskOf(json('GET', `/v1/agent/tasks/${enc(id)}`));
/** Only what changes while the model writes, for asking several times a second. */
export const agentTaskBrief = async (id: string) =>
	(await json<{ task: AgentTaskRow & { writing?: string } }>('GET', `/v1/agent/tasks/${enc(id)}?brief=true`)).task;
export const startAgentTask = (body: { folder: string; prompt: string; mode: AgentMode; web: boolean; connectors?: boolean; model?: string }) =>
	taskOf(json('POST', '/v1/agent/tasks', body));
export const tellAgentTask = (id: string, body: { text: string; mode: AgentMode; web: boolean; connectors?: boolean }) =>
	taskOf(json('POST', `/v1/agent/tasks/${enc(id)}/message`, body));
export const answerAgentTask = (id: string, call: string, allow: boolean, always = false) =>
	taskOf(json('POST', `/v1/agent/tasks/${enc(id)}/approve`, { call, allow, always }));
export const undoAgentTask = (id: string) => taskOf(json('POST', `/v1/agent/tasks/${enc(id)}/undo`));
export const stopAgentTask = (id: string) => taskOf(json('DELETE', `/v1/agent/tasks/${enc(id)}`));
export const removeAgentTask = (id: string) => json<{ removed: string }>('DELETE', `/v1/agent/tasks/${enc(id)}?remove=true`);
export const agentSkills = (folder: string) => optional<{ skills: AgentSkill[]; dir: string; folder_dir: string }>('GET', `/v1/agent/skills?folder=${enc(folder)}`);

/** Asks for a folder with the system's own dialog. `null` when cancelled, or outside the app. */
export async function pickFolder(title = 'Choose a folder'): Promise<string | null> {
	if (!isTauri) return null;
	const { invoke } = await import('@tauri-apps/api/core');
	return (await invoke<string | null>('pick_folder', { title })) ?? null;
}

// --- connectors: tools from outside that the person adds (see daemon/src/connectors.rs) ----------

export interface ConnectorParam {
	name: string;
	type: string;
	required: boolean;
}

export interface ConnectorTool {
	name: string;
	description: string;
	params: ConnectorParam[];
	/** The tool's own schema for its arguments. */
	schema: { properties?: Record<string, { type?: string | string[]; description?: string }>; required?: string[] };
	/** Runs without asking the person first. */
	always: boolean;
}

export interface Connector {
	id: string;
	name: string;
	description: string;
	/** A program on this computer, or a web address. */
	kind: 'stdio' | 'http';
	command: string;
	args: string[];
	/** The names of its environment variables. Their values never leave the service. */
	env: string[];
	cwd: string;
	command_line: string;
	url: string;
	has_token: boolean;
	always_allow: string[];
	on: boolean;
	state: 'stopped' | 'starting' | 'running' | 'failed';
	error: string | null;
	/** How many tools it has, when it runs. */
	tools: number;
	/** What the connector says it is, in one line. */
	about: string;
}

/** A ready-made connector: it only fills in the form. */
export interface ReadyConnector {
	id: string;
	name: string;
	does: string;
	command: string;
	args: string[];
	needs: 'node' | 'python';
}

/** What is sent to add or change a connector. A variable with `null` keeps its saved value; no `token` keeps the saved token. */
export interface ConnectorBody {
	name: string;
	description: string;
	kind: 'stdio' | 'http';
	command?: string;
	args?: string[];
	env?: Record<string, string | null>;
	cwd?: string;
	url?: string;
	token?: string;
	always_allow: string[];
}

export interface ConnectorList {
	connectors: Connector[];
	gallery: ReadyConnector[];
	/** Which of the programs that fetch and run a connector are installed. */
	programs: { npx: boolean; uvx: boolean };
}

/** `null` from a service that is too old to have connectors. */
export const connectors = () => optional<ConnectorList>('GET', '/v1/connectors');
export const addConnector = (body: ConnectorBody) => json<Connector>('POST', '/v1/connectors', body);
export const saveConnector = (id: string, body: ConnectorBody) => json<Connector>('PUT', `/v1/connectors/${enc(id)}`, body);
export const deleteConnector = (id: string) => json<{ deleted: string }>('DELETE', `/v1/connectors/${enc(id)}`);
export const startConnector = (id: string) => json<Connector>('POST', `/v1/connectors/${enc(id)}/start`);
export const stopConnector = (id: string) => json<Connector>('POST', `/v1/connectors/${enc(id)}/stop`);
export const connectorTools = async (id: string) => (await json<{ tools: ConnectorTool[] }>('GET', `/v1/connectors/${enc(id)}/tools`)).tools;
/** Calls one tool. `from: 'chat'` words the answer for a model; without it the answer is the tool's own text. */
export const callConnector = (id: string, tool: string, args: unknown, from?: 'chat') =>
	json<{ ok: boolean; text: string }>('POST', `/v1/connectors/${enc(id)}/call`, { tool, arguments: args, ...(from ? { from } : {}) });
export const allowConnectorTool = (id: string, tool: string, always: boolean) => json<Connector>('POST', `/v1/connectors/${enc(id)}/allow`, { tool, always });
/** The three tools that fit a few words best, across the running connectors, as text for a model. */
export const findConnectorTool = (query: string) => json<{ text: string; matches: { connector: string; tool: string }[] }>('POST', '/v1/connectors/find', { query });

// --- skills: instructions for one kind of work (see daemon/src/agent/skills.rs) -------------------

export interface Skill {
	name: string;
	description: string;
	source: 'built-in' | 'yours' | 'folder';
	enabled: boolean;
	/** Where it is offered: everywhere, only in Code, or only in the chat. */
	where: 'any' | 'code' | 'chat';
	/** The person's own skill that stands in for one that comes with Rigbrew. */
	replaces_built_in: boolean;
}

export interface SkillDetail extends Skill {
	body: string;
	licence: string | null;
	files: { path: string; bytes: number }[];
}

/** A skill that could be brought in from a folder or from a repository, as it is shown before saving. */
export interface FoundSkill {
	name: string;
	description: string;
	licence: string | null;
	bytes: number;
	files: string[];
	/** Why it cannot be brought in, when it cannot. */
	problem: string | null;
	at: string;
	replaces_built_in: boolean;
}

const folderQuery = (folder?: string) => (folder ? `?folder=${enc(folder)}` : '');
/** `null` from a service that is too old to manage skills. */
export const skills = (folder?: string) => optional<{ skills: Skill[]; dir: string }>('GET', `/v1/skills${folderQuery(folder)}`);
export const skill = (name: string, folder?: string) => json<SkillDetail>('GET', `/v1/skills/${enc(name)}${folderQuery(folder)}`);
export const createSkill = (body: { name: string; description: string; body: string; copy_of?: string }) => json<Skill>('POST', '/v1/skills', body);
export const saveSkill = (was: string, body: { name: string; description: string; body: string }) => json<Skill>('PUT', `/v1/skills/${enc(was)}`, body);
export const setSkillEnabled = (name: string, enabled: boolean) => json<Skill>('PUT', `/v1/skills/${enc(name)}`, { enabled });
export const deleteSkill = (name: string) => json<{ deleted: string }>('DELETE', `/v1/skills/${enc(name)}`);
/** What `use_skill` answers in the chat. */
export const useSkill = (name: string) => json<{ ok: boolean; text: string }>('POST', `/v1/skills/${enc(name)}/use`);
export const skillFile = (name: string, path: string) => json<{ ok: boolean; text: string }>('GET', `/v1/skills/${enc(name)}/file?path=${enc(path)}`);
/** Looks at the skills in a folder or at a repository address; with `save`, saves the ones named. */
export const importSkills = (from: 'folder' | 'github', location: string, save?: string[]) =>
	json<{ found: FoundSkill[]; saved: string[] }>('POST', '/v1/skills/import', { from, location, ...(save ? { save } : {}) });
