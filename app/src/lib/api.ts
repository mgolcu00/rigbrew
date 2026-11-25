// Talks to rigbrewd, the local service, directly over HTTP on 127.0.0.1.
// Inside the app the port and token come from the native side, which also starts the service
// again when it is gone. In a plain browser (pnpm dev) they come from the dev server, which
// reads daemon.json, or from VITE_RIGBREW_PORT and VITE_RIGBREW_TOKEN.
//
// The port and token change every time the service starts. When a request cannot reach the
// service, this module forgets the connection, tells the app (`onLink`), and keeps asking for
// a new one until the service answers again.

import type { Activity, AppConfig, Consent, Job, Package, QueuedEvent, Session, System, Usage } from './types';

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

/** Saves a JSON text the person asked to keep (the account export), the same way. */
export async function saveText(name: string, text: string): Promise<'saved' | 'downloaded' | 'cancelled'> {
	if (isTauri) {
		const { invoke } = await import('@tauri-apps/api/core');
		const path = await invoke<string | null>('save_text', { name, text });
		return path ? 'saved' : 'cancelled';
	}
	const url = URL.createObjectURL(new Blob([text], { type: 'application/json' }));
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

// --- Code: tasks a model works on in a folder (the loop runs in the service) ------------------

export type AgentMode = 'ask' | 'read_only' | 'auto_edits';
export type AgentState = 'running' | 'waiting' | 'done' | 'failed' | 'stopped' | 'interrupted';

export interface AgentToolStep {
	kind: 'tool';
	call: string;
	name: string;
	arguments: Record<string, unknown>;
	status: 'running' | 'waiting' | 'done' | 'failed' | 'denied' | 'stopped';
	result: string | null;
	diff: string | null;
}

export type AgentStep = { kind: 'user' | 'model' | 'note'; text: string } | AgentToolStep;

/** A step that waits for the person: a change to a file (with its diff) or a command. */
export interface AgentPending {
	call: string;
	tool: string;
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
}

export interface AgentTask extends AgentTaskRow {
	web: boolean;
	steps: AgentStep[];
	pending: AgentPending | null;
	error: string | null;
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
export const startAgentTask = (body: { folder: string; prompt: string; mode: AgentMode; web: boolean; model?: string }) => taskOf(json('POST', '/v1/agent/tasks', body));
export const tellAgentTask = (id: string, body: { text: string; mode: AgentMode; web: boolean }) => taskOf(json('POST', `/v1/agent/tasks/${enc(id)}/message`, body));
export const answerAgentTask = (id: string, call: string, allow: boolean, always = false) =>
	taskOf(json('POST', `/v1/agent/tasks/${enc(id)}/approve`, { call, allow, always }));
export const stopAgentTask = (id: string) => taskOf(json('DELETE', `/v1/agent/tasks/${enc(id)}`));
export const removeAgentTask = (id: string) => json<{ removed: string }>('DELETE', `/v1/agent/tasks/${enc(id)}?remove=true`);
export const agentSkills = (folder: string) => optional<{ skills: AgentSkill[]; dir: string; folder_dir: string }>('GET', `/v1/agent/skills?folder=${enc(folder)}`);

/** Asks for a folder with the system's own dialog. `null` when cancelled, or outside the app. */
export async function pickFolder(): Promise<string | null> {
	if (!isTauri) return null;
	const { invoke } = await import('@tauri-apps/api/core');
	return invoke<string | null>('pick_folder');
}
