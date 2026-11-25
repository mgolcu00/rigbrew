import { agentPrompt, allowedTools, type Agent, type ToolName } from './agents';
import { agents } from './agents.svelte';
import * as api from './api';
import { artifactsPrompt, asksForPiece } from './artifacts';
import { ux } from './experience.svelte';
import { installedVariant } from './format';
import { guide } from './guide.svelte';
import { app } from './state.svelte';
import type { Package } from './types';
import { nameOf } from './words';
import { looksEnglish, reader } from './voice.svelte';

/** Where the next message goes: to the chat model, or straight to the image or video model. */
export type Mode = 'chat' | 'image' | 'video';

export type SizeName = 'square' | 'portrait' | 'landscape';

/** Width and height per shape. The service wants multiples of 16. */
export const sizes: Record<SizeName, { label: string; width: number; height: number }> = {
	square: { label: 'Square', width: 1024, height: 1024 },
	portrait: { label: 'Portrait', width: 832, height: 1216 },
	landscape: { label: 'Landscape', width: 1216, height: 832 }
};

export const IMAGE_STEPS = 8;

/** Clip shapes. The video model wants multiples of 32. */
export const videoSizes: Record<SizeName, { label: string; width: number; height: number }> = {
	landscape: { label: 'Landscape', width: 832, height: 480 },
	portrait: { label: 'Portrait', width: 480, height: 832 },
	square: { label: 'Square', width: 640, height: 640 }
};

const shapeLabels: Record<SizeName, string> = { landscape: 'Landscape', portrait: 'Portrait', square: 'Square' };

/** The clip shapes of a video package: its own (the manifest's `defaults.sizes`), else the usual three. */
export function videoSizesOf(pkg: Package | null | undefined): Record<SizeName, { label: string; width: number; height: number }> {
	const own = pkg?.defaults?.sizes;
	if (!own) return videoSizes;
	const out = { ...videoSizes };
	for (const name of Object.keys(shapeLabels) as SizeName[]) if (own[name]) out[name] = { label: shapeLabels[name], width: own[name][0], height: own[name][1] };
	return out;
}

/** The clip lengths of a video package: its own, else the usual three. */
export const videoLengthsOf = (pkg: Package | null | undefined): readonly { frames: number; label: string }[] => (pkg?.defaults?.lengths?.length ? pkg.defaults.lengths : videoLengths);

/** Clip lengths: 4k+1 frames at 24 frames a second. Longer clips take disproportionately longer. */
export const videoLengths = [
	{ frames: 33, label: '1.4 s' },
	{ frames: 49, label: '2 s' },
	{ frames: 73, label: '3 s' }
] as const;

export const VIDEO_STEPS = 20;

export interface VideoJob {
	prompt: string;
	width: number;
	height: number;
	frames: number;
	steps: number;
	seed: number;
	package: string;
	packageName: string;
	state: 'waiting' | 'done' | 'failed' | 'stopped';
	/** What the engine is doing: loading, conditioning, sampling, vae_decode, encoding. */
	stage: string;
	step: number | null;
	totalSteps: number | null;
	startedAt: number;
	waited?: number;
	seconds?: number;
	variant?: string;
	/** The service's job id while the clip is being made. */
	jobId?: string;
	/** The saved clip: `<id>.webm` under `/v1/videos/files/`. */
	file?: string;
	error?: string;
}

export interface ToolCall {
	id: string;
	name: string;
	/** JSON text, as the model wrote it. */
	arguments: string;
}

export interface ImageJob {
	prompt: string;
	width: number;
	height: number;
	steps: number;
	seed: number;
	package: string;
	packageName: string;
	/** mode: typed in Image mode. tool: the chat model asked. regenerate: the card's own button. */
	via: 'mode' | 'tool' | 'regenerate';
	/** Name of the chat model that asked, when via is "tool". */
	askedBy?: string;
	state: 'waiting' | 'done' | 'failed' | 'stopped';
	/** True when another engine was in memory when this started: the wait includes a swap. */
	switching: boolean;
	startedAt: number;
	/** How long the wait lasted when it ended without a picture (stopped or failed). */
	waited?: number;
	/** Generation time, as the service measured it. */
	seconds?: number;
	variant?: string;
	/** While waiting: steps done and how many, as the engine reports them. */
	step?: number | null;
	totalSteps?: number | null;
	/** The picture itself, in memory only for this session; never saved with the chat. */
	base64?: string;
	/** The PNG the service saved (`/v1/images/files/<id>.png`): what a saved chat keeps. */
	imageId?: string;
	error?: string;
	/** The tool call this answers, real or made up for the history (see `history`). */
	callId: string;
}

export interface Message {
	id: number;
	/** image: a picture card. video: a clip card. offer: "install an image model" in place of a picture. */
	role: 'user' | 'assistant' | 'image' | 'video' | 'offer';
	content: string;
	/** User only: which mode it was sent in. */
	mode?: Mode;
	/** Assistant only. waiting: no token yet (the model may still be loading). */
	state?: 'waiting' | 'streaming' | 'done' | 'stopped' | 'failed';
	/** When it was said (user) or asked for (assistant). */
	at?: number;
	/** Assistant only: seconds from the request to the end of the answer. */
	took?: number;
	/** Assistant only: another engine was in memory when the wait started. */
	switching?: boolean;
	startedAt?: number;
	error?: string;
	tokensPerSecond?: number;
	tokens?: number;
	/** Assistant only: who answered, for the readout. */
	modelName?: string;
	variant?: string;
	toolCalls?: ToolCall[];
	/** Assistant only: what each tool call that left no card answered (read aloud, video), by call id. */
	toolResults?: Record<string, string>;
	/** Assistant only: said by the read_aloud tool; the text is what was spoken. */
	spoken?: boolean;
	image?: ImageJob;
	video?: VideoJob;
	/** Offer only: what was asked for, so it can be made once the install is done. */
	offer?: { prompt: string; size: SizeName; callId?: string };
}

export interface Conversation {
	/** Also the id the service stores it under: `[A-Za-z0-9_-]{1,64}`. */
	id: string;
	title: string;
	messages: Message[];
	startedAt: number;
	updatedAt: number;
	/** False for a saved chat whose messages have not been read from the service yet. */
	loaded: boolean;
	/** Set when reading it from the service failed. */
	problem?: string;
	/** The agent this chat runs as (see `agents.ts`); none for the plain chat model. */
	agentId?: string;
}

/** What is stored with each chat (`data` in `PUT /v1/chats/{id}`). Pictures are ids, not pixels. */
interface Stored {
	v: 1;
	startedAt: number;
	messages: Message[];
	/** The id of the agent the chat used, when it used one. */
	agentId?: string;
}

/** What the engine is doing for the app right now, for the bay. */
export interface Work {
	/** loading: nothing was in memory. switching: another model is unloaded first. */
	kind: 'loading' | 'switching' | 'generating';
	package: string;
	since: number;
}

const imageTool = {
	type: 'function',
	function: {
		name: 'generate_image',
		description: 'Create one image from a text description, using the image model installed on this machine. The image is shown to the user directly.',
		parameters: {
			type: 'object',
			properties: {
				prompt: { type: 'string', description: 'What the image shows: subject, setting, style, light. In English, one to three sentences.' },
				size: { type: 'string', enum: ['square', 'portrait', 'landscape'], description: 'Shape of the image. Default square.' }
			},
			required: ['prompt']
		}
	}
};

// Who the model is, said on every turn: without it the model answers with the name it was trained under.
const identityPrompt =
	'You are Steep, the chat model of Rigbrew, a free open-source app that runs AI on the user\'s own computer. You are running locally on this machine. ' +
	'If asked who or what you are, or which model you are, say that you are Steep, Rigbrew\'s chat model, built from an open-weight model and running on this computer. ' +
	'Do not give any other company, product or model name as your identity.';

const speakTool = {
	type: 'function',
	function: {
		name: 'read_aloud',
		description:
			'Speak a text aloud to the user with the voice model installed on this machine. Use it when the user asks for audio, sound, speech, a voice, narration, or to hear, say, pronounce or read something out. This is the only tool that makes sound.',
		parameters: {
			type: 'object',
			properties: { text: { type: 'string', description: 'Exactly the words to speak, as plain text without Markdown. English only.' } },
			required: ['text']
		}
	}
};

const videoTool = {
	type: 'function',
	function: {
		name: 'generate_video',
		description: 'Make one short video clip from a text description, using the video model installed on this machine. It takes minutes; the clip appears in the chat when it is ready.',
		parameters: {
			type: 'object',
			properties: { prompt: { type: 'string', description: 'What the clip shows: subject, motion, setting, light. In English, one to three sentences.' } },
			required: ['prompt']
		}
	}
};

/** What the chat model is told about the tools it has on this turn, and which one this message reads like. */
function toolsPrompt(names: ToolName[], hint: ToolName | null): string {
	const lines = ['You have these tools; pick by what the user asks for, and answer in text when none fits.'];
	if (names.includes('generate_image')) lines.push('generate_image: only for a still picture, drawing, photo, illustration or logo.');
	if (names.includes('read_aloud')) lines.push('read_aloud: only for sound: audio, speech, a voice, narration, saying or reading something out. Never use generate_image for sound.');
	if (names.includes('generate_video')) lines.push('generate_video: only for a video, clip or animation. Never use generate_image for a video.');
	lines.push('A web page, a document, a diagram, code or an SVG is text you write yourself in a code block: never call a tool for those.');
	lines.push('After a tool returns, its result is already in front of the user: reply with one short sentence and do not describe or link it.');
	if (hint && names.includes(hint)) lines.push(`The latest message reads like a request for ${hint === 'read_aloud' ? 'sound' : hint === 'generate_video' ? 'a video' : 'a picture'}: use ${hint}.`);
	return lines.join(' ');
}

const asksForSound = (text: string) =>
	/\b(read\s+(it|this|that|me)?\s*(aloud|out)|say\s+(it|this|that)|speak|voice|audio|sound|pronounce|narrat\w*|text[- ]to[- ]speech|tts)\b/i.test(text) ||
	/(sesli|seslendir|ses\s+(üret|olarak|ver|yap|oluştur)|sesini|konuş)/i.test(text);

const asksForVideo = (text: string) => /\b(video|clip|animation|animate|movie|footage)\b/i.test(text) || /(klip|animasyon)/i.test(text);

/** Which tool a message plainly asks for, when exactly one kind of thing is named. */
function plainlyAsksFor(text: string): ToolName | null {
	const wants: ToolName[] = [];
	if (asksForSound(text)) wants.push('read_aloud');
	if (asksForVideo(text)) wants.push('generate_video');
	if (asksForImage(text) || /(resim|görsel|fotoğraf|çiz)/i.test(text)) wants.push('generate_image');
	return wants.length === 1 ? wants[0] : null;
}

/** Rough check for "this is a request for a picture", used only when no image model is installed. */
const asksForImage = (text: string) =>
	/^\s*(please\s+)?(draw|paint|sketch|illustrate)\b/i.test(text) ||
	/\b(draw|paint|sketch|generate|make|create|render|design|give)\b[^.?!]{0,60}\b(image|picture|photo|photograph|illustration|drawing|painting|logo|icon|wallpaper|poster)\b/i.test(text);

// --- saved pictures ------------------------------------------------------------------------------

// A saved picture needs the token, so it cannot be a bare <img src>: it is fetched once and
// shown through an object URL, kept for as long as the app runs.
const pictureUrls = new Map<string, Promise<string>>();

export function pictureUrl(id: string): Promise<string> {
	let url = pictureUrls.get(id);
	if (!url) {
		url = api.imageFile(id).then((blob) => URL.createObjectURL(blob));
		url.catch(() => pictureUrls.delete(id));
		pictureUrls.set(id, url);
	}
	return url;
}

function forgetPicture(id: string) {
	const url = pictureUrls.get(id);
	pictureUrls.delete(id);
	url?.then((u) => URL.revokeObjectURL(u)).catch(() => {});
}

/** The PNG as base64, for saving it: from memory, or read back from the service. */
export async function pictureBase64(job: ImageJob): Promise<string> {
	if (job.base64) return job.base64;
	if (!job.imageId) return '';
	const blob = await api.imageFile(job.imageId);
	return new Promise((resolve, reject) => {
		const reader = new FileReader();
		reader.onload = () => resolve(String(reader.result).replace(/^data:[^,]*,/, ''));
		reader.onerror = () => reject(reader.error);
		reader.readAsDataURL(blob);
	});
}

let nextId = 1;
const randomSeed = () => Math.floor(Math.random() * 2 ** 31);
const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));

const clipUrls = new Map<string, Promise<string>>();

/** A saved clip as an object URL, fetched once per session. */
export function clipUrl(file: string): Promise<string> {
	let url = clipUrls.get(file);
	if (!url) {
		url = api.videoFile(file).then((blob) => URL.createObjectURL(blob));
		url.catch(() => clipUrls.delete(file));
		clipUrls.set(file, url);
	}
	return url;
}
const newChatId = () => `c${Date.now().toString(36)}${Math.random().toString(36).slice(2, 8)}`;
const titleOf = (text: string) => text.replace(/\s+/g, ' ').trim().slice(0, 64);

/** How long a deleted chat can be brought back before it is deleted for good. */
const UNDO_MS = 6000;

/**
 * Conversations. Each one is saved by the service (`/v1/chats`) after every completed turn,
 * so they outlive the app; with an older service that cannot save them they live in memory.
 */
class ChatState {
	conversations = $state<Conversation[]>([]);
	/** '' is a new, still empty chat. */
	activeId = $state('');
	/** Whether chats are saved: unknown until the list was asked for, 'no' with an older service. */
	persist = $state<'unknown' | 'yes' | 'no'>('unknown');
	/** The last save that failed, said quietly next to the chat list. */
	saveProblem = $state('');
	/** For screen readers: the answer that just finished, read out politely once. */
	announce = $state('');
	model = $state('');
	mode = $state<Mode>('chat');
	videoSize = $state<SizeName>('landscape');
	videoFrames = $state<number>(49);
	size = $state<SizeName>('square');
	busy = $state(false);
	/** Bumped to ask the composer to take focus. */
	focusRequest = $state(0);
	/** The engine the service holds in memory (it keeps one), as last heard from it. */
	engine = $state<{ package: string; variant: string } | null>(null);
	/** What is being waited for; null when idle. */
	work = $state<Work | null>(null);
	/** Tokens per second of the answer being written, measured as it arrives. */
	rate = $state(0);
	/** The speed of the last finished answer, kept on the bay until the next one. */
	lastRate = $state(0);
	/** Why the last turn failed; cleared by the next one. */
	failure = $state('');

	active = $derived(this.conversations.find((c) => c.id === this.activeId) ?? null);

	/** The agent picked for a chat that has not started yet. */
	#nextAgent = $state('');
	/** The id of the agent this chat runs as; '' is the plain chat model. */
	agentId = $derived(this.active ? (this.active.agentId ?? '') : this.#nextAgent);
	/** That agent, or null: none picked, or it was deleted since. */
	agent = $derived(agents.byId(this.agentId));

	/** Picks the agent for this chat ('' for none). `from`: picker, sheet or new (just made). */
	setAgent(id: string, from: string) {
		if (this.busy || id === this.agentId) return;
		const conversation = this.active;
		if (!conversation) {
			this.#nextAgent = id;
		} else {
			if (!conversation.loaded) return;
			conversation.agentId = id || undefined;
			this.saveSoon(conversation, 0);
		}
		// The id only: never the name or the instruction.
		if (id) app.track('agent_used', { agent: id, from });
	}

	#abort: AbortController | null = null;

	/** Every model the service has in memory, the one used last at the end. */
	resident = $state<{ package: string; variant: string }[]>([]);

	/** Whether `id` is in memory now, so using it needs no load or swap. */
	inMemory(id: string) {
		return this.engine?.package === id || this.resident.some((r) => r.package === id);
	}

	/** Whether `pkg` can be loaded next to the model of the other kind that is in memory (chat beside picture). */
	#fitsBeside(pkg: Package) {
		const budget = app.system?.accel_budget_mb ?? 0;
		const need = installedVariant(pkg)?.accel_mem_mb ?? Number.POSITIVE_INFINITY;
		const chatKind = pkg.modality === 'chat';
		let used = 0;
		for (const r of this.resident) {
			const other = app.packages.find((p) => p.id === r.package);
			if (!other || (other.modality === 'chat') === chatKind) continue;
			used += other.variants.find((v) => v.id === r.variant)?.accel_mem_mb ?? 0;
		}
		return used + need <= budget;
	}

	/** Whether using `pkg` takes another model out of memory first. */
	#swaps(pkg: Package) {
		return this.engine !== null && !this.inMemory(pkg.id) && !this.#fitsBeside(pkg);
	}

	/** Asks the service what is loaded. Called when the app opens and after each turn. */
	async syncEngine() {
		try {
			const all = await api.engines();
			this.resident = all.map((e) => ({ package: e.package, variant: e.variant }));
			const last = all[all.length - 1];
			if (!this.busy) this.engine = last ? { package: last.package, variant: last.variant } : null;
		} catch {
			// An older service without this endpoint: the readout fills in after the first answer.
		}
	}

	/** Takes the model out of memory. The next message loads it again. */
	async unload() {
		if (this.busy) return;
		await api.unloadEngine();
		this.engine = null;
		this.resident = [];
	}

	/**
	 * Follows an engine swap for an image job: once the service reports the image model as
	 * loaded, the card can say "generating" instead of "switching".
	 */
	async #followSwap(job: ImageJob, signal: AbortSignal) {
		while (job.state === 'waiting' && job.switching && !signal.aborted) {
			try {
				const loaded = await api.engine(signal);
				if (loaded?.package === job.package && job.state === 'waiting') {
					job.switching = false;
					this.work = { kind: 'generating', package: job.package, since: Date.now() };
					this.engine = { package: loaded.package, variant: loaded.variant };
					return;
				}
			} catch {
				return;
			}
			await new Promise((r) => setTimeout(r, 1500));
		}
	}

	newChat() {
		this.stop();
		// A new chat starts as the plain chat model; one that is still empty keeps what was picked for it.
		if (this.activeId) this.#nextAgent = '';
		this.activeId = '';
		this.focusRequest++;
	}

	/** `from`: pane, palette, recent (the empty chat's list), or '' when the app itself opens it. */
	async open(id: string, from = '') {
		if (id === this.activeId) return;
		if (from) app.track('chat_opened', { from });
		this.stop();
		this.activeId = id;
		this.focusRequest++;
		const conversation = this.conversations.find((c) => c.id === id);
		if (conversation && !conversation.loaded) await this.#hydrate(conversation);
	}

	// --- saved chats ---------------------------------------------------------------------------------

	#saveTimers = new Map<string, ReturnType<typeof setTimeout>>();
	#trash = new Map<string, { conversation: Conversation; at: number; timer: ReturnType<typeof setTimeout> }>();

	/** Reads the list of saved chats. Called when the app opens and after the service restarted. */
	async loadList() {
		let rows: api.ChatRow[] | null;
		try {
			rows = await api.chats();
		} catch {
			return; // the service is away; the link listener calls this again
		}
		if (rows === null) {
			this.persist = 'no';
			return;
		}
		this.persist = 'yes';
		void agents.load();
		for (const row of rows) {
			if (this.#trash.has(row.id)) continue;
			const known = this.conversations.find((c) => c.id === row.id);
			if (known) {
				if (!known.loaded) Object.assign(known, { title: row.title, updatedAt: row.updated_ms });
				continue;
			}
			this.conversations.push({ id: row.id, title: row.title, messages: [], startedAt: row.created_ms, updatedAt: row.updated_ms, loaded: false });
		}
		this.#sort();
	}

	#sort() {
		this.conversations.sort((a, b) => b.updatedAt - a.updatedAt);
	}

	/** Reads one saved chat's messages from the service. */
	async #hydrate(conversation: Conversation) {
		conversation.problem = undefined;
		try {
			const saved = await api.chatById(conversation.id);
			const data = saved?.data as Stored | undefined;
			const messages = Array.isArray(data?.messages) ? data.messages : [];
			for (const m of messages) {
				nextId = Math.max(nextId, (m.id ?? 0) + 1);
				// Anything that was still running when it was saved did not finish.
				if (m.state === 'waiting' || m.state === 'streaming') m.state = 'stopped';
				if (m.image?.state === 'waiting') m.image.state = 'stopped';
			}
			conversation.messages = messages;
			conversation.startedAt = data?.startedAt ?? conversation.startedAt;
			conversation.agentId = typeof data?.agentId === 'string' && data.agentId ? data.agentId : undefined;
			conversation.loaded = true;
			// A clip may still be on its way: the service makes it whether or not the app is watching.
			for (const m of conversation.messages) if (m.video?.state === 'waiting') this.#follow(conversation, m.video);
		} catch (e) {
			conversation.problem = e instanceof Error ? e.message : String(e);
		}
	}

	/** Tries reading a chat again after it failed. */
	retryOpen(id: string) {
		const conversation = this.conversations.find((c) => c.id === id);
		if (conversation) this.#hydrate(conversation);
	}

	/** Saves a chat soon: after a turn, a rename. Several changes in a row are one save. */
	saveSoon(conversation: Conversation, delay = 600) {
		conversation.updatedAt = Date.now();
		this.#sort();
		if (this.persist === 'no') return;
		clearTimeout(this.#saveTimers.get(conversation.id));
		this.#saveTimers.set(
			conversation.id,
			setTimeout(() => {
				this.#saveTimers.delete(conversation.id);
				this.#save(conversation);
			}, delay)
		);
	}

	async #save(conversation: Conversation) {
		if (!conversation.loaded || this.#trash.has(conversation.id)) return;
		// Pixels stay out of the saved chat: a picture is kept as the id of the file the service saved.
		const messages = $state.snapshot(conversation.messages).map((m) => (m.image ? { ...m, image: { ...m.image, base64: undefined } } : m));
		const data: Stored = { v: 1, startedAt: conversation.startedAt, messages, ...(conversation.agentId ? { agentId: conversation.agentId } : {}) };
		try {
			await api.saveChat(conversation.id, conversation.title, data);
			this.persist = 'yes';
			this.saveProblem = '';
		} catch (e) {
			if (e instanceof api.ApiError && e.status === 404) {
				this.persist = 'no';
				return;
			}
			this.saveProblem = e instanceof Error ? e.message : String(e);
		}
	}

	rename(id: string, title: string) {
		const conversation = this.conversations.find((c) => c.id === id);
		const clean = titleOf(title);
		if (!conversation || !clean || clean === conversation.title) return;
		conversation.title = clean;
		this.saveSoon(conversation, 0);
		app.track('chat_renamed');
	}

	/** Deletes a chat after a few seconds; until then Undo brings it back. */
	remove(id: string) {
		const at = this.conversations.findIndex((c) => c.id === id);
		if (at < 0) return;
		const conversation = this.conversations[at];
		if (this.activeId === id) this.newChat();
		clearTimeout(this.#saveTimers.get(id));
		this.#saveTimers.delete(id);
		this.conversations.splice(at, 1);
		const timer = setTimeout(() => this.#purge(id), UNDO_MS);
		app.track('chat_deleted');
		this.#trash.set(id, { conversation, at, timer });
		app.say(`Deleted “${conversation.title}”.`, { label: 'Undo', run: () => this.undoRemove(id) }, UNDO_MS);
	}

	undoRemove(id: string) {
		const entry = this.#trash.get(id);
		if (!entry) return;
		clearTimeout(entry.timer);
		this.#trash.delete(id);
		this.conversations.splice(Math.min(entry.at, this.conversations.length), 0, entry.conversation);
		app.track('chat_delete_undone');
		app.say(`“${entry.conversation.title}” is back.`);
	}

	/** Deletes a chat for good: its pictures first, then the chat. */
	async #purge(id: string) {
		const entry = this.#trash.get(id);
		if (!entry) return;
		const { conversation } = entry;
		try {
			if (!conversation.loaded) await this.#hydrate(conversation);
			const pictures = conversation.messages.map((m) => m.image?.imageId).filter((x): x is string => Boolean(x));
			await Promise.all(pictures.map((p) => api.deleteImageFile(p).catch(() => {})));
			pictures.forEach(forgetPicture);
			if (this.persist !== 'no') await api.deleteChat(id);
		} catch (e) {
			app.say(`Could not delete “${conversation.title}”. ${e instanceof Error ? e.message : String(e)}`);
		} finally {
			this.#trash.delete(id);
		}
	}

	/** Deletes every saved chat and its pictures (Settings). */
	async removeAll(): Promise<number> {
		this.newChat();
		const all = [...this.conversations];
		this.conversations = [];
		for (const conversation of all) {
			if (!conversation.loaded) await this.#hydrate(conversation);
			for (const m of conversation.messages) {
				const picture = m.image?.imageId;
				if (picture) {
					await api.deleteImageFile(picture).catch(() => {});
					forgetPicture(picture);
				}
			}
		}
		if (this.persist !== 'no') await api.deleteAllChats();
		return all.length;
	}

	stop() {
		if (!this.#abort) return;
		this.#abort.abort();
		this.#abort = null;
		app.track('chat_stopped', { package: this.model });
	}

	setMode(mode: Mode, via: 'click' | 'shortcut' | 'slash') {
		if (mode === this.mode) return;
		this.mode = mode;
		app.track('mode_changed', { mode, via });
		this.focusRequest++;
	}

	/** Sends what was typed, in the current mode. */
	async send(text: string) {
		const prompt = text.trim();
		if (!prompt || this.busy) return;
		const mode = this.mode;
		if (mode === 'chat' && !this.model) return;

		const conversation = this.#conversation(prompt);
		if (!conversation) return;
		conversation.messages.push({ id: nextId++, role: 'user', content: prompt, mode, at: Date.now() });

		if (mode === 'video') {
			if (!app.videoPackage) return;
			await this.#video(conversation, prompt);
			return;
		}

		if (mode === 'image') {
			if (!app.imagePackage) {
				conversation.messages.push({ id: nextId++, role: 'offer', content: '', offer: { prompt, size: this.size } });
				this.saveSoon(conversation);
				return;
			}
			await this.#turn(conversation, async (signal) => {
				await this.#image(conversation, { prompt, size: this.size, via: 'mode' }, signal);
			});
			return;
		}

		guide.count('asked');
		// No image model yet and this reads like a request for a picture: offer the install
		// here, in place, instead of letting the chat model say it cannot draw.
		const agent = agents.byId(conversation.agentId);
		if (!app.imagePackage && app.imageOffer && asksForImage(prompt) && (!agent || agent.tools.includes('generate_image'))) {
			conversation.messages.push({ id: nextId++, role: 'offer', content: '', offer: { prompt, size: 'square' } });
			this.saveSoon(conversation);
			return;
		}
		await this.#turn(conversation, (signal) => this.#chat(conversation, signal));
	}

	/** The same prompt and size again, with a new seed. */
	async regenerate(job: ImageJob) {
		const conversation = this.active;
		if (!conversation || this.busy || !app.imagePackage) return;
		const size = (Object.keys(sizes) as SizeName[]).find((s) => sizes[s].width === job.width && sizes[s].height === job.height) ?? 'square';
		await this.#turn(conversation, async (signal) => {
			await this.#image(conversation, { prompt: job.prompt, size, via: 'regenerate' }, signal);
		});
	}

	/** After an install offered in the chat finished: make the picture that was asked for. */
	async fulfil(message: Message) {
		const conversation = this.active;
		if (!conversation || this.busy || !app.imagePackage || !message.offer) return;
		const { prompt, size } = message.offer;
		await this.#turn(conversation, async (signal) => {
			await this.#image(conversation, { prompt, size, via: 'mode' }, signal);
		});
	}

	/** Asks the chat model again after an answer failed; the failed one stays as it was. */
	async retry() {
		const conversation = this.active;
		if (!conversation || this.busy || !this.model) return;
		await this.#turn(conversation, (signal) => this.#chat(conversation, signal));
	}

	/** Sends the last request to the chat model after all (the "answer in text" way out of an offer). */
	async answerInText() {
		const conversation = this.active;
		if (!conversation || this.busy || !this.model) return;
		await this.#turn(conversation, (signal) => this.#chat(conversation, signal, false));
	}

	#conversation(prompt: string): Conversation | null {
		// A saved chat whose messages are not read yet takes nothing: saving it would lose them.
		if (this.activeId) return this.active?.loaded ? this.active : null;
		const now = Date.now();
		const conversation: Conversation = { id: newChatId(), title: titleOf(prompt) || 'Untitled chat', messages: [], startedAt: now, updatedAt: now, loaded: true, agentId: this.#nextAgent || undefined };
		this.#nextAgent = '';
		this.conversations.unshift(conversation);
		this.activeId = conversation.id;
		app.track('chat_started', { package: this.mode === 'image' ? app.imagePackage?.id : this.model, mode: this.mode });
		return this.conversations[0];
	}

	async #turn(conversation: Conversation, work: (signal: AbortSignal) => Promise<void>) {
		const abort = new AbortController();
		this.#abort = abort;
		this.busy = true;
		this.failure = '';
		try {
			await work(abort.signal);
		} finally {
			if (this.#abort === abort) this.#abort = null;
			this.busy = false;
			this.work = null;
			this.rate = 0;
			// Every completed turn is saved, stopped and failed ones too.
			this.saveSoon(conversation);
			this.syncEngine();
		}
	}

	/** Says a tip the first time a model has to be moved into memory in place of another. */
	#switchTip(pkg: Package) {
		guide.show({
			id: 'switch',
			text: `Moving ${nameOf(pkg, ux.flags)} into memory. Only one model fits in memory at a time; switching takes a few seconds.`,
			help: 'one-model'
		});
	}

	/**
	 * The conversation as the chat model should see it. Every picture appears as a
	 * generate_image tool call with its result, whether the model asked for it or the person
	 * used Image mode, so the model always knows what is on screen and how it got there.
	 */
	#history(conversation: Conversation, tools: ToolName[], agent: Agent | null) {
		const withTools = tools.length > 0;
		const out: Record<string, unknown>[] = [];
		const last = conversation.messages.findLast((m) => m.role === 'user')?.content ?? '';
		// Who it is, then the agent's instruction when the chat runs as one, then how to write things to keep, then the tools.
		const system = [identityPrompt];
		if (agent) system.push(agentPrompt(agent));
		system.push(artifactsPrompt);
		if (withTools) system.push(toolsPrompt(tools, plainlyAsksFor(last)));
		out.push({ role: 'system', content: system.join(' ') });
		const answered = new Set<string>();
		const result = (m: Message): string => {
			if (m.role === 'offer') return 'No image model is installed. The user was offered the install.';
			const job = m.image!;
			if (job.state === 'done') return `Done. A ${job.width}x${job.height} image is now shown to the user.`;
			if (job.state === 'failed') return `The image could not be made: ${job.error ?? 'unknown error'}`;
			return 'The user stopped the image before it was finished.';
		};
		for (const [i, m] of conversation.messages.entries()) {
			if (m.role === 'user') {
				out.push({ role: 'user', content: m.content });
			} else if (m.role === 'assistant') {
				if (m.state === 'failed' || m.state === 'waiting' || (!m.content && !m.toolCalls?.length)) continue;
				if (!withTools || !m.toolCalls?.length) {
					if (m.content) out.push({ role: 'assistant', content: m.content });
					continue;
				}
				out.push({
					role: 'assistant',
					content: m.content,
					tool_calls: m.toolCalls.map((c) => ({ id: c.id, type: 'function', function: { name: c.name, arguments: c.arguments } }))
				});
				// Each call needs an answer right after it. The card (or offer) that followed holds it.
				for (const call of m.toolCalls) {
					const card = conversation.messages.slice(i + 1).find((x) => (x.image?.callId ?? x.offer?.callId) === call.id);
					out.push({ role: 'tool', tool_call_id: call.id, content: card ? result(card) : (m.toolResults?.[call.id] ?? 'That tool is not available.') });
					answered.add(call.id);
				}
			} else if (withTools && m.role === 'image' && m.image && m.image.state !== 'waiting' && !answered.has(m.image.callId)) {
				const { callId, prompt } = m.image;
				out.push({ role: 'assistant', content: '', tool_calls: [{ id: callId, type: 'function', function: { name: 'generate_image', arguments: JSON.stringify({ prompt }) } }] });
				out.push({ role: 'tool', tool_call_id: callId, content: result(m) });
			}
		}
		return out;
	}

	/** One request to the chat model, and, when it calls the image tool, the picture and a follow-up. */
	async #chat(conversation: Conversation, signal: AbortSignal, allowTools = true) {
		const pkg = app.chatPackages.find((p) => p.id === this.model);
		if (!pkg) return;
		// The model may ask for an image when one can be made here, or at least installed.
		const available: ToolName[] = [];
		if (allowTools) {
			if (app.imagePackage ?? app.imageOffer) available.push('generate_image');
			if (app.voicePackage) available.push('read_aloud');
			if (app.videoPackage) available.push('generate_video');
		}
		// A chat that runs as an agent is offered only what that agent may use.
		const agent = agents.byId(conversation.agentId);
		const asked = conversation.messages.findLast((m) => m.role === 'user')?.content ?? '';
		// A page, a diagram or a document is written, not made by a tool: on a turn that asks
		// for one and does not plainly ask for sound or a video, no tool is offered at all ("a fox as SVG" is written too).
		const wants = plainlyAsksFor(asked);
		const tools = asksForPiece(asked) && wants !== 'read_aloud' && wants !== 'generate_video' ? [] : allowedTools(agent, available);

		for (let round = 0; round < 3; round++) {
			conversation.messages.push({
				id: nextId++,
				role: 'assistant',
				content: '',
				state: 'waiting',
				switching: this.#swaps(pkg),
				startedAt: Date.now(),
				at: Date.now(),
				modelName: pkg.name,
				variant: installedVariant(pkg)?.id
			});
			const reply = conversation.messages[conversation.messages.length - 1];
			this.work = { kind: reply.switching ? 'switching' : this.inMemory(pkg.id) ? 'generating' : 'loading', package: pkg.id, since: Date.now() };
			// The other engine is on its way out; until an answer arrives nothing is known to be loaded.
			if (reply.switching) {
				this.engine = null;
				this.#switchTip(pkg);
			}
			const calls = await this.#stream(pkg, reply, this.#history(conversation, tools, agent), tools, signal);
			if (reply.state === 'done' && reply.content) {
				const text = reply.content.replace(/[#*_`>|]/g, '').replace(/\s+/g, ' ').trim();
				this.announce = `${nameOf(pkg, ux.flags)} answered: ${text.length > 600 ? `${text.slice(0, 600)}…` : text}`;
			}
			if (reply.state !== 'done' || calls.length === 0) return;

			reply.toolCalls = calls;
			// Whether the model gets another turn: after a picture, or when a tool could not do its work.
			let again = false;
			const results: Record<string, string> = (reply.toolResults = {});
			for (const call of calls) {
				let args: { prompt?: unknown; size?: unknown; text?: unknown } = {};
				try {
					args = JSON.parse(call.arguments || '{}');
				} catch {
					// Malformed arguments: fall through to the "nothing asked" case.
				}
				app.track('chat_tool_called', { tool: call.name, package: pkg.id });
				if (call.name === 'read_aloud') {
					const text = typeof args.text === 'string' ? args.text.trim() : '';
					const voice = app.voicePackage;
					if (!text || !voice) {
						results[call.id] = voice ? 'Nothing to say: the text was empty.' : 'No voice model is installed. Tell the user to add Voice under Models.';
						again = true;
						continue;
					}
					// The words appear as an answer and are spoken at once; Read aloud under it plays them again.
					conversation.messages.push({ id: nextId++, role: 'assistant', content: text, state: 'done', at: Date.now(), modelName: pkg.name, spoken: true });
					const said = conversation.messages[conversation.messages.length - 1];
					if (!looksEnglish(text)) {
						// The voice reads English only: the words stay on screen and the model says why they were not spoken.
						results[call.id] = 'The text is on screen but was not spoken: the voice model reads English only for now. Tell the user that in one short sentence, in their language.';
						again = true;
						continue;
					}
					results[call.id] = 'Done. The text is on screen and is being spoken to the user.';
					void reader.toggle(said.id, voice.id, text).then(() => {
						if (reader.error) app.say(`Could not read it aloud. ${reader.error}`);
					});
					continue;
				}
				if (call.name === 'generate_video') {
					const prompt = typeof args.prompt === 'string' ? args.prompt.trim() : '';
					if (!prompt || !app.videoPackage) {
						results[call.id] = app.videoPackage ? 'Nothing to make: the description was empty.' : 'No video model is installed. Tell the user to add Video under Models.';
						again = true;
						continue;
					}
					results[call.id] = 'Started. The clip is being made in the background and appears in the chat when it is ready.';
					await this.#video(conversation, prompt);
					continue;
				}
				if (call.name !== 'generate_image') {
					again = true;
					continue;
				}
				const prompt = typeof args.prompt === 'string' ? args.prompt.trim() : '';
				const size: SizeName = typeof args.size === 'string' && args.size in sizes ? (args.size as SizeName) : 'square';
				if (!prompt) continue;
				if (!app.imagePackage) {
					conversation.messages.push({ id: nextId++, role: 'offer', content: '', offer: { prompt, size, callId: call.id } });
					return;
				}
				const job = await this.#image(conversation, { prompt, size, via: 'tool', callId: call.id, askedBy: pkg.name }, signal);
				if (job.state !== 'done') return;
				again = true;
			}
			// Let the model finish its turn now that the tool has answered.
			if (!again) return;
		}
	}

	/** Streams one completion into `reply` and returns the tool calls the model made. */
	async #stream(pkg: Package, reply: Message, messages: unknown[], tools: ToolName[], signal: AbortSignal): Promise<ToolCall[]> {
		const calls: ToolCall[] = [];
		let firstTokenAt = 0;
		let pieces = 0;
		try {
			const response = await api.chatStream(
				{
					model: pkg.id,
					messages,
					stream: true,
					stream_options: { include_usage: true },
					chat_template_kwargs: { enable_thinking: false },
					...(tools.length ? { tools: tools.map((t) => (t === 'generate_image' ? imageTool : t === 'read_aloud' ? speakTool : videoTool)) } : {})
				},
				signal
			);
			const reader = response.body!.pipeThrough(new TextDecoderStream()).getReader();
			let buffer = '';
			for (;;) {
				const { value, done } = await reader.read();
				if (done) break;
				buffer += value;
				let newline: number;
				while ((newline = buffer.indexOf('\n')) >= 0) {
					const line = buffer.slice(0, newline).trim();
					buffer = buffer.slice(newline + 1);
					if (!line.startsWith('data:')) continue;
					const data = line.slice(5).trim();
					if (data === '[DONE]') continue;
					let chunk: any;
					try {
						chunk = JSON.parse(data);
					} catch {
						continue;
					}
					if (chunk.error) throw new Error(chunk.error.message ?? 'The model stopped with an error.');
					// The engine names the file it loaded: "...steep-4b-v1.q5.gguf".
					const build = typeof chunk.model === 'string' ? chunk.model.match(/\.([a-z0-9]+)\.gguf$/i)?.[1] : undefined;
					if (build) reply.variant = build;
					if (this.engine?.package !== pkg.id || (build && this.engine.variant !== build)) {
						this.engine = { package: pkg.id, variant: build ?? reply.variant ?? '' };
					}
					const delta = chunk.choices?.[0]?.delta;
					const piece: string | undefined = delta?.content;
					if (piece) {
						if (!firstTokenAt) {
							firstTokenAt = performance.now();
							this.work = { kind: 'generating', package: pkg.id, since: Date.now() };
						}
						pieces++;
						const span = (performance.now() - firstTokenAt) / 1000;
						if (pieces > 4 && span > 0.25) this.rate = pieces / span;
						reply.content += piece;
						reply.state = 'streaming';
					}
					// Tool calls arrive in pieces too: the name first, then the arguments as text.
					for (const part of delta?.tool_calls ?? []) {
						const at = part.index ?? 0;
						calls[at] ??= { id: '', name: '', arguments: '' };
						if (part.id) calls[at].id = part.id;
						if (part.function?.name) calls[at].name += part.function.name;
						if (part.function?.arguments) calls[at].arguments += part.function.arguments;
					}
					// llama-server reports its own timing on the last chunk.
					if (chunk.timings?.predicted_per_second) {
						reply.tokensPerSecond = chunk.timings.predicted_per_second;
						reply.tokens = chunk.timings.predicted_n;
					} else if (chunk.usage?.completion_tokens && !reply.tokens) {
						reply.tokens = chunk.usage.completion_tokens;
					}
				}
			}
			reply.state = 'done';
		} catch (e) {
			if (signal.aborted) {
				reply.state = 'stopped';
			} else if (e instanceof TypeError) {
				// The stream broke in the middle: the service went away while answering.
				api.reportLost();
				reply.state = 'failed';
				reply.error = api.LOST;
			} else {
				reply.state = 'failed';
				reply.error = e instanceof Error ? e.message : String(e);
			}
			if (reply.state === 'failed') this.failure = reply.error ?? 'No answer.';
		}
		reply.took = reply.startedAt ? (Date.now() - reply.startedAt) / 1000 : undefined;
		// When the engine gave no timing, fall back to what arrived over the wire.
		const seconds = firstTokenAt ? (performance.now() - firstTokenAt) / 1000 : 0;
		if (!reply.tokensPerSecond && pieces > 1 && seconds > 0.2) {
			reply.tokens ??= pieces;
			reply.tokensPerSecond = reply.tokens / seconds;
		}
		if (reply.tokensPerSecond) this.lastRate = reply.tokensPerSecond;
		const made = calls.filter(Boolean).filter((c) => c.name);
		made.forEach((c, i) => (c.id ||= `call_${reply.id}_${i}`));
		return reply.state === 'done' ? made : [];
	}

	/** Adds a picture card to the conversation and fills it when the image model answers. */
	/** Starts one clip. The service makes it in the background; `#follow` keeps the card current. */
	async #video(conversation: Conversation, prompt: string) {
		const pkg = app.videoPackage!;
		const { width, height } = videoSizesOf(pkg)[this.videoSize];
		// A length the package does not offer (the choice was made for another one) falls back to its own default.
		const lengths = videoLengthsOf(pkg);
		const frames = lengths.some((l) => l.frames === this.videoFrames) ? this.videoFrames : (pkg.defaults?.frames ?? lengths[0].frames);
		conversation.messages.push({
			id: nextId++,
			role: 'video',
			content: '',
			video: {
				prompt,
				width,
				height,
				frames,
				steps: pkg.defaults?.steps ?? VIDEO_STEPS,
				seed: randomSeed(),
				package: pkg.id,
				packageName: pkg.name,
				state: 'waiting',
				stage: 'loading',
				step: null,
				totalSteps: null,
				startedAt: Date.now()
			}
		});
		const job = conversation.messages[conversation.messages.length - 1].video!;
		if (this.engine && this.engine.package !== pkg.id) this.#switchTip(pkg);
		this.engine = null;
		app.track('video_requested', { package: pkg.id, width, height, frames: job.frames, steps: job.steps });
		try {
			job.jobId = await api.startVideo({ model: pkg.id, prompt, size: `${width}x${height}`, frames: job.frames, steps: job.steps, seed: job.seed });
		} catch (e) {
			job.state = 'failed';
			job.error = e instanceof Error ? e.message : String(e);
			job.waited = (Date.now() - job.startedAt) / 1000;
			this.failure = job.error;
			return;
		}
		// Saved now, so a reload or another chat does not lose the card; it is picked up again.
		this.saveSoon(conversation, 0);
		this.#follow(conversation, job);
	}

	#following = new Set<string>();
	/** How many clips are being followed: keeps the clocks on the cards ticking. */
	videosRunning = $state(0);

	/** Follows a clip until it is done, stopped or failed. Survives switching chats and reloads. */
	async #follow(conversation: Conversation, job: VideoJob) {
		const id = job.jobId;
		if (!id || this.#following.has(id)) return;
		this.#following.add(id);
		this.videosRunning = this.#following.size;
		const pkg = app.packages.find((p) => p.id === job.package);
		try {
			for (;;) {
				let s: api.VideoJobState;
				try {
					s = await api.videoJob(id);
				} catch (e) {
					if (e instanceof api.ApiError && e.status === 404) {
						// The service was restarted (Rigbrew quit) and forgot the job.
						job.state = 'failed';
						job.error = 'Rigbrew was closed while this clip was being made.';
						break;
					}
					await pause(3000);
					continue;
				}
				job.stage = s.stage;
				job.step = s.step;
				job.totalSteps = s.steps;
				if (s.variant) job.variant = s.variant;
				if (s.state === 'loading' || s.state === 'running') {
					if (!this.busy) this.work = { kind: s.state === 'loading' ? 'loading' : 'generating', package: job.package, since: this.work?.since ?? Date.now() };
					await pause(1500);
					continue;
				}
				if (s.state === 'done') {
					job.file = s.video?.path.split('/').pop();
					job.seconds = s.elapsed_ms / 1000;
					job.state = 'done';
					this.engine = { package: job.package, variant: s.variant };
					this.announce = `Video ready: ${job.prompt}`;
					guide.show({ id: 'video', text: 'This clip is saved with the chat. Save keeps a copy where you choose.', help: 'pictures' });
				} else if (s.state === 'cancelled') {
					job.state = 'stopped';
				} else {
					job.state = 'failed';
					job.error = s.error ?? 'The video was not made.';
				}
				break;
			}
		} finally {
			this.#following.delete(id);
			this.videosRunning = this.#following.size;
			if (job.state !== 'done') job.waited = (Date.now() - job.startedAt) / 1000;
			if (!this.busy && this.work?.package === job.package) this.work = null;
			app.track(job.state === 'done' ? 'video_made' : `video_${job.state}`, { package: pkg?.id ?? job.package, seconds: Math.round((Date.now() - job.startedAt) / 1000), frames: job.frames });
			this.saveSoon(conversation);
		}
	}

	/** Stops a clip that is being made; the engine stops at its next step. */
	async stopVideo(job: VideoJob) {
		if (job.jobId) await api.stopVideo(job.jobId);
	}

	async #image(
		conversation: Conversation,
		ask: { prompt: string; size: SizeName; via: ImageJob['via']; callId?: string; askedBy?: string },
		signal: AbortSignal
	): Promise<ImageJob> {
		const pkg = app.imagePackage!;
		const { width, height } = sizes[ask.size];
		const id = nextId++;
		conversation.messages.push({
			id,
			role: 'image',
			content: '',
			image: {
				prompt: ask.prompt,
				width,
				height,
				steps: IMAGE_STEPS,
				seed: randomSeed(),
				package: pkg.id,
				packageName: pkg.name,
				via: ask.via,
				askedBy: ask.askedBy,
				state: 'waiting',
				switching: this.#swaps(pkg),
				startedAt: Date.now(),
				callId: ask.callId ?? `call_image_${id}`
			}
		});
		const job = conversation.messages[conversation.messages.length - 1].image!;
		this.work = { kind: job.switching ? 'switching' : this.inMemory(pkg.id) ? 'generating' : 'loading', package: pkg.id, since: Date.now() };
		if (job.switching) {
			if (this.engine) this.#switchTip(pkg);
			this.engine = null;
			this.#followSwap(job, signal);
		}
		app.track('image_requested', { package: pkg.id, via: job.via, width, height, steps: job.steps });
		const ticker = setInterval(async () => {
			const p = await api.imageProgress().catch(() => null);
			if (job.state !== 'waiting' || !p) return;
			if (p.stage === 'sampling') {
				job.step = p.completed;
				job.totalSteps = p.total;
			} else if (p.stage === 'vae_decode' || p.stage === 'encoding') {
				job.step = job.totalSteps ?? job.steps;
			}
		}, 1000);
		try {
			const answer = await api.generateImage({ model: pkg.id, prompt: job.prompt, size: `${width}x${height}`, steps: job.steps, seed: job.seed }, signal);
			const base64 = answer.data?.[0]?.b64_json;
			if (!base64) throw new Error('The image model returned no image.');
			job.base64 = base64;
			job.imageId = answer.image?.id;
			job.seconds = answer.seconds;
			job.variant = answer.variant;
			job.state = 'done';
			this.engine = { package: pkg.id, variant: answer.variant };
			guide.count('pictures');
			this.announce = `Picture ready: ${job.prompt}`;
			const name = nameOf(pkg, ux.flags);
			if (job.via === 'tool') {
				guide.show({
					id: 'toolpicture',
					text: `${ask.askedBy ? (ux.flags.plainNames ? `Chat (${ask.askedBy})` : ask.askedBy) : 'The chat model'} asked ${name} to make this picture. Picture mode, in the box below, goes straight there and is quicker.`,
					help: 'pictures'
				});
			}
			guide.show({
				id: 'picture',
				text: `${job.imageId ? 'This picture is saved with the chat.' : 'This picture stays until Rigbrew quits.'} Save keeps a copy where you choose; Try again makes a new one from the same words.`,
				help: 'pictures'
			});
		} catch (e) {
			if (signal.aborted) {
				job.state = 'stopped';
			} else {
				job.state = 'failed';
				job.error = e instanceof Error ? e.message : String(e);
				this.failure = job.error;
			}
			job.waited = (Date.now() - job.startedAt) / 1000;
			// The swap may or may not have happened: nothing is known to be loaded.
			this.engine = null;
		} finally {
			clearInterval(ticker);
		}
		return job;
	}
}

export const chat = new ChatState();
