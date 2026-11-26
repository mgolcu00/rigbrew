// The Studio workstation's state: the film that is open, what is selected, where the playhead
// is, the undo history, and every action the screen offers. The panels read it and call it;
// none of them talks to the service on its own.

import * as api from './api.ts';
import type { StudioBrief, StudioJob, StudioListing, StudioPace, StudioShot, StudioSound, StudioView } from './api.ts';
import { drawCaption } from './studio-captions.ts';
import { codeOf, plain } from './errors.ts';
import { remembered } from './remember.ts';
import { app } from './state.svelte.ts';
import { MASTER, SHOTS, StudioAudio, type PlayRegion } from './studio-audio.ts';
import {
	AssistTurn,
	History,
	docMember,
	docOf,
	docShot,
	heard,
	names,
	moveShots,
	newId,
	overlapping,
	pasteRegions,
	regionEnd,
	saveBody,
	splitRegion,
	stripGain,
	takeWork,
	type Doc,
	type DocLine,
	type DocMember,
	type DocRegion,
	type DocShot,
	type DocTrack
} from './studio-edit.ts';
import { filmPlan, hasClip, shotSeconds } from './studio.ts';
import { FPS, MAX_PPS, MIN_PPS, snapFrame } from './studio-time.ts';

export { MASTER, SHOTS };

/** The row of the pictures, which is no strip of the mixer. */
export const FILM = 'film';

/** The colours a new track gets, in turn (the same as the service gives). */
export const TRACK_COLORS = ['#4f8fd9', '#3fae8a', '#c98a3c', '#b06bc9', '#d0605e', '#5fa8b8', '#9aa63f', '#8d7fd6'];

/** The peaks of a sound: lowest and highest of every fiftieth of a second, each -127 to 127. */
export interface Peaks {
	rate: number;
	seconds: number;
	pairs: Int8Array;
}

interface Layout {
	library: boolean;
	inspector: boolean;
	bottom: boolean;
	tab: 'mixer' | 'editor' | 'cast';
	/** What the right column shows under the viewer: the Inspector or the assistant. */
	side: 'inspector' | 'assistant';
	libraryWidth: number;
	inspectorWidth: number;
	bottomHeight: number;
	viewerHeight: number;
	headWidth: number;
	libraryTab: 'films' | 'takes' | 'sounds';
	lcd: 'time' | 'shot';
	snap: boolean;
	vzoom: number;
	/** A track made taller or lower than the others, by its id: 1 is as the others are. */
	heights: Record<string, number>;
}

const usualLayout: Layout = {
	library: true,
	inspector: true,
	bottom: false,
	tab: 'mixer',
	side: 'inspector',
	libraryWidth: 220,
	inspectorWidth: 300,
	bottomHeight: 260,
	viewerHeight: 190,
	headWidth: 214,
	libraryTab: 'films',
	lcd: 'time',
	snap: true,
	vzoom: 1,
	heights: {}
};

const say = (e: unknown) => `${plain(e).text} ${codeOf(e)}`;
const tail = (path: string) => path.split(/[\\/]/).pop() ?? path;

/** What is asked before something that cannot simply be taken back, or that costs work. */
export interface Question {
	text: string;
	yes: string;
	no: string;
	act: () => void;
}

class Studio {
	// --- the films this install knows --------------------------------------------------------------
	films = $state<StudioListing[]>([]);
	home = $state('');
	pace = $state<StudioPace | null>(null);
	carries = $state(false);
	/** How many voices the installed voice has. */
	voices = $state(0);

	// --- the film that is open ----------------------------------------------------------------------
	/** The film as the service last sent it: states, takes, progress, export. */
	view = $state<StudioView | null>(null);
	/** The film as it is edited here. */
	doc = $state<Doc | null>(null);
	id = $state('');
	problem = $state('');
	/** One quiet line about what just happened ("Undo: Trim shot"). */
	note = $state('');
	question = $state<Question | null>(null);
	busy = $state<'' | 'writing' | 'narrating' | 'rewriting' | 'adding' | 'looking' | 'portrait'>('');
	/** Long work on the film as the service reports it: a video brought in, a dub, clips listened to. */
	jobs = $state<{ import?: StudioJob; dub?: StudioJob; listen?: StudioJob }>({});
	/** The cast member the Cast panel shows. */
	member = $state('');
	/** The portraits that are here, by film, member and which portrait it is. */
	portraits = $state<Record<string, string>>({});

	// --- what is selected -------------------------------------------------------------------------
	shotsSel = $state<string[]>([]);
	regionsSel = $state<string[]>([]);
	/** The row the keys M and S act on: 'film', 'shots' or a track's id. */
	track = $state<string>(FILM);
	/** A track that is selected as a thing itself (its header was clicked): the Inspector shows it. */
	trackSel = $state(false);

	// --- the transport ----------------------------------------------------------------------------
	playing = $state(false);
	cycle = $state({ on: false, from: 0, to: 0 });
	/** The playhead in seconds. Not reactive: it moves sixty times a second. See `onFrame`. */
	time = 0;
	/** Changes when the playhead jumps while the film stands still, for what shows the moment in words. */
	jumps = $state(0);
	#listeners = new Set<(time: number) => void>();
	#frame = 0;
	#beat: ReturnType<typeof setInterval> | undefined;
	audio = new StudioAudio();

	// --- the look of the window -------------------------------------------------------------------
	layout = $state<Layout>({ ...usualLayout, ...remembered.json<Partial<Layout>>('studio.layout', {}) });
	/** Pixels a second on the timeline. */
	pps = $state(60);
	/** Asked by Z and by opening a film: the tracks area fits the film to its width. */
	fits = $state(0);
	viewerLarge = $state(false);
	keysOpen = $state(false);

	// --- pictures and waveforms -------------------------------------------------------------------
	/** Changes when a picture or a waveform arrives, so the lanes are drawn again. */
	drawn = $state(0);
	posters = new Map<string, HTMLImageElement>();
	peaks = new Map<string, Peaks>();
	takeUrls = $state<Record<string, string>>({});
	sounds = $state.raw<StudioSound[]>([]);
	/** Sound files that could not be read or played, by name. */
	silent = $state<string[]>([]);
	#asked = new Set<string>();
	#queue: (() => Promise<void>)[] = [];
	#working = 0;
	#clips = new Map<string, Promise<string | null>>();

	// --- undo -------------------------------------------------------------------------------------
	#histories = new Map<string, History<Doc>>();
	#before: Doc | null = null;
	/** Changes with every edit, undo and redo, for the buttons that say what can be undone. */
	edits = $state(0);
	#timer: ReturnType<typeof setTimeout> | undefined;
	#dirty = false;
	/** While the service works on the film itself, nothing is saved over what it does. */
	#held = 0;
	#saving: Promise<void> = Promise.resolve();
	#poll: ReturnType<typeof setInterval> | undefined;
	clipboard: { regions: DocRegion[]; track: string } | null = null;
	/** The request the assistant is working on, while it works: see `assistantBegin`. */
	#turn: AssistTurn | null = null;
	#assisting: Promise<void> = Promise.resolve();

	// --- what follows from the film ---------------------------------------------------------------
	project = $derived(this.view?.project ?? null);
	status = $derived(Object.fromEntries((this.view?.project.shots ?? []).map((s) => [s.id, s])) as Record<string, StudioShot>);
	/** The shots as edited here with what the service knows about their clips. */
	shots = $derived<StudioShot[]>((this.doc?.shots ?? []).map((s) => this.#whole(s)));
	/** Every shot at its place: the timeline. */
	plan = $derived(filmPlan(this.shots, true));
	/** The made shots only: what an export holds. */
	exportPlan = $derived(filmPlan(this.shots));
	/** Where the last thing on the timeline ends. */
	length = $derived(Math.max(this.plan.total, ...(this.doc?.tracks ?? []).flatMap((t) => t.regions.map(regionEnd)), 0));
	todo = $derived(this.shots.filter((s) => s.state !== 'done' && s.prompt.trim()).length);
	strips = $derived([{ id: SHOTS, ...(this.doc?.shot_track ?? { mute: false, solo: false, volume: 1, pan: 0 }) }, ...(this.doc?.tracks ?? [])]);
	heard = $derived(heard(this.strips));
	overlaps = $derived(new Set((this.doc?.tracks ?? []).flatMap((t) => [...overlapping(t.regions)])));
	/** The stretches of the film in which a line is heard: the shots' own sound goes down under them. */
	said = $derived<[number, number][]>((this.doc?.tracks ?? []).filter((t) => t.kind === 'voice' && this.heard[t.id]).flatMap((t) => t.regions.map((r) => [r.start, regionEnd(r)] as [number, number])));
	/** The subtitles on the timeline, when the film shows them. */
	cues = $derived(this.doc?.subtitles.show ? (this.view?.cues ?? []) : []);
	/** The language the film's lines are written in, and the one that is shown. */
	ownLanguage = $derived(this.doc?.language || this.view?.own_language || 'English');
	language = $derived(this.doc?.dub || this.ownLanguage);
	/** Whether the video model is installed: without it nothing is written or made. */
	canMake = $derived(Boolean(app.videoPackage));
	canWrite = $derived(Boolean(app.videoPackage) && app.chatPackages.length > 0);

	#whole(s: DocShot): StudioShot {
		const known = this.status[s.id];
		return {
			...s,
			state: known?.state ?? 'empty',
			error: known?.error,
			seconds: known?.seconds,
			made_frames: known?.made_frames,
			takes: known?.takes,
			take: known?.take,
			from: s.from
		};
	}

	shot(id: string) {
		return this.shots.find((s) => s.id === id);
	}

	docShot(id: string) {
		return this.doc?.shots.find((s) => s.id === id);
	}

	trackOf(id: string) {
		return this.doc?.tracks.find((t) => t.id === id);
	}

	/** A region and the track it is on. */
	region(id: string): { track: DocTrack; region: DocRegion } | null {
		for (const track of this.doc?.tracks ?? []) {
			const region = track.regions.find((r) => r.id === id);
			if (region) return { track, region };
		}
		return null;
	}

	/** Seconds of a sound file, once its waveform is here. */
	fileSeconds(file: string) {
		return this.peaks.get(`file:${file}`)?.seconds;
	}

	// --- films ------------------------------------------------------------------------------------

	async refresh() {
		try {
			const all = await api.studioList();
			this.films = all.projects;
			this.home = all.home ?? '';
			this.pace = all.pace ?? null;
			this.carries = Boolean(all.can_continue);
			this.voices = all.voices ?? 0;
		} catch (e) {
			this.problem = say(e);
		}
	}

	async open(id: string) {
		this.problem = '';
		try {
			await this.saveNow();
			this.adopt(await api.studioRead(id));
		} catch (e) {
			this.problem = say(e);
		}
	}

	async openFolder(folder: string, name = '') {
		this.problem = '';
		try {
			await this.saveNow();
			this.adopt(await api.studioOpen(folder, name));
			this.refresh();
		} catch (e) {
			this.problem = say(e);
		}
	}

	async forget(id: string) {
		try {
			await api.studioForget(id);
			if (this.id === id) this.close();
			await this.refresh();
		} catch (e) {
			this.problem = say(e);
		}
	}

	/** Takes a film from the service as the film that is open. */
	adopt(view: StudioView) {
		this.pause();
		const other = view.project.id !== this.id;
		this.view = view;
		this.id = view.project.id;
		this.doc = docOf(view.project);
		if (other) {
			this.#release();
			this.shotsSel = [];
			this.regionsSel = [];
			this.track = FILM;
			this.trackSel = false;
			this.time = 0;
			this.cycle = { on: false, from: 0, to: 0 };
			this.question = null;
			this.silent = [];
			this.fits++;
			this.loadSounds();
			app.track('studio_opened', { shots: view.project.shots.length, tracks: view.project.tracks?.length ?? 0 });
		}
		this.jumps++;
		this.watch();
	}

	async close() {
		this.#release();
		await this.saveNow();
		this.pause();
		this.view = null;
		this.doc = null;
		this.id = '';
		this.viewerLarge = false;
		clearInterval(this.#poll);
		this.#poll = undefined;
		this.refresh();
	}

	/** While shots are being made or the film is exported, look again every two seconds. */
	watch() {
		this.#poll ??= setInterval(async () => {
			if (!this.view || !(this.view.rendering || this.view.export?.state === 'working') || this.#dirty) return;
			try {
				const id = this.id;
				const fresh = await api.studioRead(id);
				if (this.id === id && !this.#dirty) this.view = fresh;
			} catch {
				// The service is restarting; the next look will say.
			}
		}, 2000);
	}

	/** The screen is left: the film keeps being made, the sound stops. */
	leave() {
		this.pause();
		this.audio.stopPreview();
		clearInterval(this.#poll);
		this.#poll = undefined;
		this.saveNow();
	}

	// --- edits and undo -----------------------------------------------------------------------------

	get history() {
		let h = this.#histories.get(this.id);
		if (!h) this.#histories.set(this.id, (h = new History<Doc>(200)));
		return h;
	}

	/** One edit: the film before it is kept for undo, and the film is saved a moment later. */
	edit(label: string, change: (doc: Doc) => void, key = '') {
		if (!this.doc) return;
		this.history.push($state.snapshot(this.doc) as Doc, label, key);
		change(this.doc);
		this.#touch();
	}

	/** A drag starts: the film as it is now is what an undo will bring back. */
	begin() {
		if (this.doc) this.#before = $state.snapshot(this.doc) as Doc;
	}

	/** While dragging: the film follows the pointer, and nothing is kept or saved yet. */
	live(change: (doc: Doc) => void) {
		if (this.doc) change(this.doc);
	}

	/** The drag ended: one step in the history, when anything changed. */
	commit(label: string) {
		const before = this.#before;
		this.#before = null;
		if (!before || !this.doc || JSON.stringify(before) === JSON.stringify($state.snapshot(this.doc))) return;
		this.history.push(before, label);
		this.#touch();
	}

	/** The drag was given up (Escape): the film is as it was. */
	cancel() {
		if (this.#before) this.doc = this.#before;
		this.#before = null;
	}

	undo() {
		if (!this.doc) return;
		const back = this.history.undo($state.snapshot(this.doc) as Doc);
		if (!back) return;
		this.#jump(back.state, `Undo: ${back.label}`);
	}

	redo() {
		if (!this.doc) return;
		const again = this.history.redo($state.snapshot(this.doc) as Doc);
		if (!again) return;
		this.#jump(again.state, `Redo: ${again.label}`);
	}

	#jump(doc: Doc, note: string) {
		this.doc = doc;
		this.note = note;
		// What was selected may be gone.
		this.shotsSel = this.shotsSel.filter((id) => doc.shots.some((s) => s.id === id));
		this.regionsSel = this.regionsSel.filter((id) => doc.tracks.some((t) => t.regions.some((r) => r.id === id)));
		if (this.track !== FILM && this.track !== SHOTS && !doc.tracks.some((t) => t.id === this.track)) this.track = FILM;
		this.#touch();
		this.#takes(doc);
	}

	/** After an undo, a shot may have had another take in use: the service is asked to use it again. */
	async #takes(doc: Doc) {
		const wanted = doc.shots.map((s) => [s.id, doc.takes[s.id]] as const).filter(([id, n]) => n && this.status[id]?.take !== n && this.status[id]?.takes?.some((t) => t.n === n));
		if (wanted.length === 0) return;
		await this.saveNow();
		for (const [shot, n] of wanted) {
			try {
				this.#server(await api.studioUseTake(this.id, shot, n));
			} catch (e) {
				this.problem = say(e);
			}
		}
	}

	#touch() {
		this.edits++;
		this.#dirty = true;
		clearTimeout(this.#timer);
		this.#timer = setTimeout(() => this.saveNow(), 500);
	}

	/** Saves the film now. Saves follow each other, so an older one never lands on a newer one. */
	saveNow(): Promise<void> {
		clearTimeout(this.#timer);
		this.#timer = undefined;
		if (!this.doc || !this.#dirty || this.#held > 0) return this.#saving;
		const body = saveBody($state.snapshot(this.doc) as Doc);
		const id = this.id;
		this.#dirty = false;
		this.#saving = this.#saving.then(async () => {
			try {
				const saved = await api.studioSave(id, body);
				if (this.id === id) this.#server(saved);
			} catch (e) {
				if (this.id === id) this.problem = say(e);
			}
		});
		return this.#saving;
	}

	/** What the service says about the film. The film as it is edited here stays as it is. */
	#server(view: StudioView) {
		this.view = view;
		if (!this.doc) return;
		for (const s of view.project.shots) if (s.take) this.doc.takes[s.id] = s.take;
	}

	// --- the assistant ------------------------------------------------------------------------------

	/** What the assistant does to the film, one thing after another. */
	#assist(work: () => Promise<void>): Promise<void> {
		this.#assisting = this.#assisting.then(work).catch((e) => {
			this.problem = say(e);
		});
		return this.#assisting;
	}

	/**
	 * The assistant starts a request on the open film. The film is saved first; while it works
	 * nothing is saved over what it does, and the film as it is now is what one undo brings back.
	 */
	assistantBegin(film: string): Promise<void> {
		return this.#assist(async () => {
			if (this.#turn?.film === film || this.id !== film || !this.doc) return;
			await this.saveNow();
			if (this.id !== film || !this.doc) return;
			this.#release();
			this.#turn = new AssistTurn(film, $state.snapshot(this.doc) as Doc);
			this.#held++;
		});
	}

	/** The film as the service has it now, while the assistant works: the workstation shows it at once. */
	assistantFollow(film: string): Promise<void> {
		return this.#assist(async () => {
			const turn = this.#turn;
			if (!turn || turn.film !== film || this.id !== film || !this.doc) return;
			const view = await api.studioRead(film);
			if (this.#turn !== turn || this.id !== film || !this.doc) return;
			const next = turn.follow($state.snapshot(this.doc) as Doc, docOf(view.project), this.#dirty);
			this.view = view;
			if (!next) return;
			const sounds = JSON.stringify(next.tracks) !== JSON.stringify(this.doc.tracks);
			this.doc = next;
			this.shotsSel = this.shotsSel.filter((id) => next.shots.some((s) => s.id === id));
			this.regionsSel = this.regionsSel.filter((id) => next.tracks.some((t) => t.regions.some((r) => r.id === id)));
			this.edits++;
			this.jumps++;
			if (sounds) this.loadSounds();
		});
	}

	/** The request ended: the film as the service has it, and all of it as one step of undo. */
	async assistantEnd(film: string): Promise<void> {
		await this.assistantFollow(film);
		return this.#assist(async () => {
			const turn = this.#turn;
			if (!turn || turn.film !== film) return;
			this.#release();
			if (this.id !== film || !this.doc) return;
			if (turn.end(this.history, $state.snapshot(this.doc) as Doc)) this.edits++;
			// What was edited here while it worked is saved now, with its work in it.
			if (this.#dirty) this.#touch();
		});
	}

	/** The film is no longer held for the assistant (it ended, or another film was opened). */
	#release() {
		if (!this.#turn) return;
		this.#turn = null;
		this.#held--;
	}

	/** I shows the Inspector (in place of the assistant), and hides the column again. */
	toggleInspector() {
		if (this.layout.inspector && this.layout.side === 'assistant') this.set('side', 'inspector');
		else this.set('inspector', !this.layout.inspector);
	}

	/** A shows the assistant next to the tracks, and hides it again. */
	toggleAssistant() {
		if (this.layout.inspector && this.layout.side === 'assistant') this.set('side', 'inspector');
		else {
			this.set('side', 'assistant');
			this.set('inspector', true);
		}
	}

	// --- selection --------------------------------------------------------------------------------

	selectShot(id: string, add = false) {
		this.regionsSel = [];
		this.trackSel = false;
		this.track = FILM;
		if (add) this.shotsSel = this.shotsSel.includes(id) ? this.shotsSel.filter((x) => x !== id) : [...this.shotsSel, id];
		else if (!(this.shotsSel.length === 1 && this.shotsSel[0] === id)) this.shotsSel = [id];
	}

	selectRegion(id: string, add = false) {
		const found = this.region(id);
		if (!found) return;
		this.shotsSel = [];
		this.trackSel = false;
		this.track = found.track.id;
		if (add) this.regionsSel = this.regionsSel.includes(id) ? this.regionsSel.filter((x) => x !== id) : [...this.regionsSel, id];
		else if (!(this.regionsSel.length === 1 && this.regionsSel[0] === id)) this.regionsSel = [id];
	}

	selectTrack(id: string) {
		this.shotsSel = [];
		this.regionsSel = [];
		this.track = id;
		this.trackSel = id !== FILM;
	}

	selectNothing() {
		this.shotsSel = [];
		this.regionsSel = [];
		this.trackSel = false;
	}

	/** Everything on the row that is selected: its shots, or its regions. */
	selectAll() {
		if (!this.doc) return;
		const track = this.trackOf(this.track);
		this.trackSel = false;
		if (track) {
			this.shotsSel = [];
			this.regionsSel = track.regions.map((r) => r.id);
		} else {
			this.regionsSel = [];
			this.shotsSel = this.doc.shots.map((s) => s.id);
		}
	}

	// --- shots ------------------------------------------------------------------------------------

	/** The length a new shot gets: the video package's usual one. */
	get usualFrames() {
		return app.videoPackage?.defaults?.frames ?? 121;
	}

	addShot(at?: number, prompt = '') {
		if (!this.doc) return;
		const shot = docShot({ id: newId(), prompt, frames: this.usualFrames });
		this.edit('Add shot', (doc) => doc.shots.splice(at ?? doc.shots.length, 0, shot));
		this.selectShot(shot.id);
		app.track('studio_shot_added', { shots: this.doc.shots.length });
		return shot.id;
	}

	/** Puts shots in front of the shot at `to`. Made shots that would have to be made again are asked about first. */
	moveShotsTo(ids: string[], to: number) {
		if (!this.doc) return;
		const next = moveShots(this.shots, ids, to);
		if (next.every((s, i) => s.id === this.shots[i].id)) return;
		const now = this.shots;
		const remade = next.filter((s, at) => s.state === 'done' && s.continues && (at === 0 || now[now.indexOf(s) - 1]?.id !== next[at - 1].id));
		const act = () => {
			this.edit(ids.length === 1 ? 'Move shot' : 'Move shots', (doc) => (doc.shots = moveShots(doc.shots, ids, to)));
			app.track('studio_shot_moved', { shots: ids.length });
		};
		if (remade.length === 0) return act();
		const n = remade.length;
		this.question = {
			text: `${n === 1 ? 'One shot continues' : `${n} shots continue`} the shot before. After this move ${n === 1 ? 'it starts' : 'they start'} from a different picture and ${n === 1 ? 'has' : 'have'} to be made again.`,
			yes: 'Move',
			no: 'Leave it',
			act
		};
	}

	/** Delete: the selected shots or regions, or the selected track. Made shots are asked about once. */
	deleteSelection() {
		if (!this.doc) return;
		if (this.regionsSel.length > 0) {
			const ids = this.regionsSel;
			this.edit(ids.length === 1 ? 'Delete region' : 'Delete regions', (doc) => {
				for (const t of doc.tracks) t.regions = t.regions.filter((r) => !ids.includes(r.id));
			});
			this.regionsSel = [];
		} else if (this.shotsSel.length > 0) {
			const ids = this.shotsSel;
			const made = this.shots.filter((s) => ids.includes(s.id) && hasClip(s)).length;
			const act = () => {
				this.edit(ids.length === 1 ? 'Delete shot' : 'Delete shots', (doc) => (doc.shots = doc.shots.filter((s) => !ids.includes(s.id))));
				this.shotsSel = [];
			};
			if (made === 0) act();
			else
				this.question = {
					text: `${made === 1 ? 'This shot is made.' : `${made} of these shots are made.`} Deleting takes ${made === 1 ? 'it' : 'them'} out of the film. Undo brings ${made === 1 ? 'it' : 'them'} back with ${made === 1 ? 'its clip' : 'their clips'}.`,
					yes: 'Delete',
					no: 'Keep',
					act
				};
		} else if (this.trackSel) this.deleteTrack(this.track);
	}

	/** Cmd+D: copies of what is selected, right after it. */
	duplicateSelection() {
		if (!this.doc) return;
		if (this.regionsSel.length > 0) {
			const ids = this.regionsSel;
			const made: string[] = [];
			this.edit('Duplicate', (doc) => {
				for (const t of doc.tracks) {
					const picked = t.regions.filter((r) => ids.includes(r.id));
					if (picked.length === 0) continue;
					const copies = pasteRegions(picked, Math.max(...picked.map(regionEnd)));
					made.push(...copies.map((r) => r.id));
					t.regions.push(...copies);
				}
			});
			this.regionsSel = made;
		} else if (this.shotsSel.length > 0) {
			const ids = this.shotsSel;
			const made: string[] = [];
			this.edit('Duplicate', (doc) => {
				const last = Math.max(...ids.map((id) => doc.shots.findIndex((s) => s.id === id)));
				// A copy has no clip: it is a new shot with the same description.
				const copies = doc.shots.filter((s) => ids.includes(s.id)).map((s) => ({ ...s, id: newId(), trim_start: 0, trim_end: 0, seed: null }));
				made.push(...copies.map((s) => s.id));
				doc.shots.splice(last + 1, 0, ...copies);
			});
			this.shotsSel = made;
		} else if (this.trackSel) this.duplicateTrack(this.track);
	}

	// --- tracks -----------------------------------------------------------------------------------

	addTrack(name = ''): string {
		if (!this.doc) return '';
		const id = newId();
		const n = this.doc.tracks.length;
		this.edit('Add track', (doc) => doc.tracks.push({ id, name: name || `Sound ${n + 1}`, kind: 'sound', lang: '', color: TRACK_COLORS[n % TRACK_COLORS.length], volume: 1, pan: 0, mute: false, solo: false, regions: [] }));
		this.selectTrack(id);
		app.track('studio_track_added', { tracks: n + 1 });
		return id;
	}

	duplicateTrack(id: string) {
		const at = this.doc?.tracks.findIndex((t) => t.id === id) ?? -1;
		if (!this.doc || at < 0) return;
		const from = $state.snapshot(this.doc.tracks[at]) as DocTrack;
		const copy: DocTrack = { ...from, id: newId(), kind: 'sound', lang: '', name: `${from.name} copy`.slice(0, 40), solo: false, regions: from.regions.map((r) => ({ ...r, id: newId() })) };
		this.edit('Duplicate track', (doc) => doc.tracks.splice(at + 1, 0, copy));
		this.selectTrack(copy.id);
		app.track('studio_track_added', { tracks: this.doc.tracks.length });
	}

	deleteTrack(id: string) {
		if (!this.trackOf(id)) return;
		this.edit('Delete track', (doc) => (doc.tracks = doc.tracks.filter((t) => t.id !== id)));
		this.regionsSel = [];
		this.track = FILM;
		this.trackSel = false;
	}

	moveTrack(id: string, to: number) {
		const from = this.doc?.tracks.findIndex((t) => t.id === id) ?? -1;
		if (!this.doc || from < 0 || to === from || to === from + 1) return;
		this.edit('Move track', (doc) => {
			const [track] = doc.tracks.splice(from, 1);
			doc.tracks.splice(to > from ? to - 1 : to, 0, track);
		});
	}

	/** M and S on a strip: 'shots' or a track. */
	toggle(id: string, what: 'mute' | 'solo') {
		if (!this.doc || id === FILM) return;
		this.edit(what === 'mute' ? 'Mute' : 'Solo', (doc) => {
			const strip = id === SHOTS ? doc.shot_track : doc.tracks.find((t) => t.id === id);
			if (strip) strip[what] = !strip[what];
		});
	}

	// --- regions ----------------------------------------------------------------------------------

	/** Sets a sound file on a track at a moment of the film, as long as the file is. */
	async addRegion(track: string, file: string, start: number) {
		if (!this.doc) return;
		const seconds = (await this.peaksOf(`file:${file}`))?.seconds ?? 0;
		if (seconds <= 0) {
			this.problem = `${tail(file)} could not be read as sound.`;
			return;
		}
		const id = track && this.trackOf(track) ? track : this.addTrack();
		const region: DocRegion = { id: newId(), file, start: Math.max(0, snapFrame(start)), offset: 0, length: seconds, gain: 1, fade_in: 0, fade_out: 0, loops: 1 };
		this.edit('Add region', (doc) => doc.tracks.find((t) => t.id === id)?.regions.push(region));
		this.selectRegion(region.id);
		this.loadSound(file);
		app.track('studio_region_added', { kind: 'file' });
	}

	/** Cmd+T: cuts the selected regions (or the ones under the playhead on the selected track) at the playhead. For a made shot under the playhead it sets the trim. */
	splitAtPlayhead() {
		if (!this.doc) return;
		const at = snapFrame(this.time);
		const track = this.trackOf(this.track);
		const picked = this.regionsSel.length > 0 ? this.regionsSel : (track?.regions.filter((r) => r.start < at && regionEnd(r) > at).map((r) => r.id) ?? []);
		if (picked.length > 0) {
			let cut = 0;
			this.edit('Split', (doc) => {
				for (const t of doc.tracks) {
					t.regions = t.regions.flatMap((r) => {
						const parts = picked.includes(r.id) ? splitRegion(r, at) : null;
						if (parts) cut++;
						return parts ?? [r];
					});
				}
			});
			if (cut === 0) this.note = 'The playhead is not inside a selected region.';
			return;
		}
		// The made shot under the playhead: what lies before the playhead is cut from its start,
		// or what lies after it from its end, whichever is less.
		const part = this.plan.parts.find((p) => p.made && p.start < at && p.start + p.length > at && (this.shotsSel.length === 0 || this.shotsSel.includes(p.shot)));
		if (!part) {
			this.note = 'Put the playhead inside a region or a made shot to split it.';
			return;
		}
		const into = at - part.start;
		this.edit('Trim at playhead', (doc) => {
			const s = doc.shots.find((x) => x.id === part.shot);
			if (!s) return;
			if (into <= part.length / 2) s.trim_start = snapFrame(s.trim_start + into);
			else s.trim_end = snapFrame(s.trim_end + part.length - into);
		});
		this.selectShot(part.shot);
	}

	copy() {
		const picked = this.regionsSel.map((id) => this.region(id)).filter((x) => x !== null);
		if (picked.length === 0) return;
		this.clipboard = { regions: picked.map((x) => $state.snapshot(x.region) as DocRegion), track: picked[0].track.id };
		this.note = picked.length === 1 ? 'Copied one region.' : `Copied ${picked.length} regions.`;
	}

	paste() {
		if (!this.doc || !this.clipboard) return;
		const target = this.trackOf(this.track)?.id ?? this.trackOf(this.clipboard.track)?.id ?? this.doc.tracks[0]?.id;
		if (!target) return;
		const copies = pasteRegions(this.clipboard.regions, snapFrame(this.time));
		this.edit('Paste', (doc) => doc.tracks.find((t) => t.id === target)?.regions.push(...copies));
		this.regionsSel = copies.map((r) => r.id);
		this.shotsSel = [];
		this.track = target;
		app.track('studio_region_added', { kind: 'paste' });
	}

	// --- making, takes, export ----------------------------------------------------------------------

	async make(shots?: string[]) {
		if (!this.doc || !this.canMake) return;
		this.problem = '';
		try {
			await this.saveNow();
			this.#server(await api.studioRender(this.id, shots));
			app.track('studio_make_clicked', { shots: shots?.length ?? this.todo });
		} catch (e) {
			this.problem = say(e);
		}
	}

	async stopMaking() {
		try {
			this.#server(await api.studioStop(this.id));
		} catch (e) {
			this.problem = say(e);
		}
	}

	async useTake(shot: string, n: number) {
		if (!this.doc || this.status[shot]?.take === n) return;
		this.problem = '';
		try {
			await this.saveNow();
			const before = $state.snapshot(this.doc) as Doc;
			const view = await api.studioUseTake(this.id, shot, n);
			this.history.push(before, 'Use take');
			this.view = view;
			// A take has its own length, and the trims are held against it.
			const now = docOf(view.project);
			this.doc.takes = now.takes;
			for (const s of this.doc.shots) {
				const fresh = now.shots.find((x) => x.id === s.id);
				if (fresh) Object.assign(s, { frames: fresh.frames, trim_start: fresh.trim_start, trim_end: fresh.trim_end });
			}
			this.edits++;
			app.track('studio_take_used', { takes: this.status[shot]?.takes?.length ?? 0 });
		} catch (e) {
			this.problem = say(e);
		}
	}

	deleteTake(shot: string, n: number) {
		this.question = {
			text: `Delete take ${n}? Its clip is removed from the film's folder. This cannot be undone.`,
			yes: 'Delete',
			no: 'Keep',
			act: async () => {
				try {
					await this.saveNow();
					this.#server(await api.studioDeleteTake(this.id, shot, n));
				} catch (e) {
					this.problem = say(e);
				}
			}
		};
	}

	async exportFilm() {
		if (!this.doc || this.exportPlan.parts.length === 0) return;
		this.problem = '';
		try {
			await this.saveNow();
			if (this.doc.subtitles.burn) await this.#captions();
			await api.studioExport(this.id);
			app.track('studio_export', {
				shots: this.exportPlan.parts.length,
				tracks: this.doc.tracks.length,
				regions: this.doc.tracks.reduce((n, t) => n + t.regions.length, 0),
				scale: this.doc.export.scale === 2 ? 2 : 1,
				quality: this.doc.export.quality || 'balanced'
			});
			this.view = await api.studioRead(this.id);
		} catch (e) {
			this.problem = say(e);
			try {
				this.view = await api.studioRead(this.id);
			} catch {
				// said already
			}
		}
	}

	/**
	 * Draws the subtitles as pictures for an export that holds them in its frames, and hands
	 * them to the service: one picture for each, as wide as the exported film. Subtitles that are
	 * drawn already are not drawn again. When this does not work the export still goes on, with
	 * the subtitles as a track.
	 */
	async #captions() {
		const id = this.id;
		try {
			const sub = await api.studioSubtitles(id);
			if (sub.drawn || sub.cues.length === 0) return;
			this.note = 'Drawing the subtitles…';
			for (const [n, cue] of sub.cues.entries()) {
				const picture = drawCaption(cue.text, sub.width, sub.height);
				if (!picture) return;
				await api.studioCaption(id, sub.key, n, sub.cues.length, picture.width, picture.height, picture.rgba);
			}
		} catch {
			// The export says how the subtitles went out.
		} finally {
			if (this.note === 'Drawing the subtitles…') this.note = '';
		}
	}

	async reveal() {
		try {
			await api.studioReveal(this.id);
		} catch (e) {
			this.problem = say(e);
		}
	}

	// --- the chat model and the voice -------------------------------------------------------------

	/** The chat model writes shots from an idea: after the ones the film has, or in place of them. */
	async write(idea: string, count: number, replace: boolean) {
		if (!this.doc || !idea.trim() || this.busy) return false;
		this.busy = 'writing';
		this.problem = '';
		try {
			// The shots are written for the film as it is saved: its cast, its look, whether it speaks.
			await this.saveNow();
			const written = await api.studioStoryboard(this.id, idea.trim(), count);
			const carries = Boolean(this.view?.can_continue);
			this.edit(replace ? 'Write the shots again' : 'Write shots', (doc) => {
				if (replace) doc.shots = [];
				for (const s of written) {
					const lines = (s.lines ?? []).map((l) => ({ id: newId(), who: l.who, text: l.text }));
					doc.shots.push(docShot({ id: newId(), prompt: s.prompt, frames: this.usualFrames, continues: Boolean(s.continues && carries && doc.shots.length > 0), lines }));
				}
			});
			app.track('studio_shots_written', { shots: written.length, replace });
			return true;
		} catch (e) {
			this.problem = say(e);
			return false;
		} finally {
			this.busy = '';
		}
	}

	/** The chat model writes one description again; the old one comes back with undo. */
	async rewrite(shot: string, hint = '') {
		const s = this.docShot(shot);
		if (!s || this.busy) return;
		this.busy = 'rewriting';
		this.problem = '';
		try {
			await this.saveNow();
			const prompt = await api.studioRewrite(this.id, shot, s.prompt, hint);
			this.edit('Rewrite description', (doc) => {
				const now = doc.shots.find((x) => x.id === shot);
				if (now) now.prompt = prompt;
			});
			app.track('studio_shot_rewritten', { hint: Boolean(hint.trim()) });
		} catch (e) {
			this.problem = say(e);
		} finally {
			this.busy = '';
		}
	}

	/** The voice reads a text; the reading becomes a region at the playhead on a narration track. */
	async narrate(text: string) {
		if (!this.doc || !text.trim() || this.busy) return false;
		this.busy = 'narrating';
		this.problem = '';
		try {
			await this.saveNow();
			const before = $state.snapshot(this.doc) as Doc;
			const track = this.trackOf(this.track)?.name.toLowerCase().includes('narration') ? this.track : undefined;
			const view = await api.studioNarration(this.id, text.trim(), snapFrame(this.time), track);
			this.history.push(before, 'Narration');
			this.view = view;
			this.doc.tracks = docOf(view.project).tracks;
			this.edits++;
			this.selectRegion(view.region.id);
			this.loadSounds();
			app.track('studio_region_added', { kind: 'narration' });
			return true;
		} catch (e) {
			this.problem = say(e);
			return false;
		} finally {
			this.busy = '';
		}
	}

	// --- the cast -----------------------------------------------------------------------------------

	/** The shots that name a member, as the film stands now. */
	shotsWith(member: string): StudioShot[] {
		const who = this.doc?.cast.find((m) => m.id === member);
		return who ? this.shots.filter((s) => names(s.prompt, who.name)) : [];
	}

	addMember(name = '', look = ''): string {
		if (!this.doc) return '';
		const taken = new Set(this.doc.cast.map((m) => m.name.toLowerCase()));
		let fresh = name.trim() || 'Character';
		for (let n = 2; taken.has(fresh.toLowerCase()); n++) fresh = `${name.trim() || 'Character'} ${n}`;
		const member = docMember({ id: newId(), name: fresh, look });
		this.edit('Add character', (doc) => doc.cast.push(member));
		this.member = member.id;
		app.track('studio_cast_added', { cast: this.doc.cast.length });
		return member.id;
	}

	setMember(id: string, label: string, change: (m: DocMember) => void, key = '') {
		this.edit(
			label,
			(doc) => {
				const m = doc.cast.find((x) => x.id === id);
				if (m) change(m);
			},
			key ? `${key}:${id}` : ''
		);
	}

	removeMember(id: string) {
		if (!this.doc?.cast.some((m) => m.id === id)) return;
		this.edit('Remove character', (doc) => {
			doc.cast = doc.cast.filter((m) => m.id !== id);
			for (const s of doc.shots) {
				if (s.from === id) s.from = '';
				for (const l of s.lines) if (l.who === id) l.who = '';
			}
		});
		if (this.member === id) this.member = this.doc.cast[0]?.id ?? '';
	}

	/** The chat model writes how a member looks from a few words; the old look comes back with undo. */
	async writeLook(id: string, words: string) {
		const m = this.doc?.cast.find((x) => x.id === id);
		if (!m || this.busy) return false;
		this.busy = 'looking';
		this.problem = '';
		try {
			await this.saveNow();
			const look = await api.studioLook(this.id, m.name, words.trim() || m.look);
			this.setMember(id, 'Write the look', (x) => (x.look = look));
			app.track('studio_look_written', {});
			return true;
		} catch (e) {
			this.problem = say(e);
			return false;
		} finally {
			this.busy = '';
		}
	}

	/** The picture model makes a portrait from the member's look. */
	async makePortrait(id: string) {
		if (!this.doc || this.busy) return;
		this.busy = 'portrait';
		this.problem = '';
		try {
			await this.saveNow();
			this.#server(await api.studioMakePortrait(this.id, id));
			app.track('studio_portrait_made', {});
		} catch (e) {
			this.problem = say(e);
		} finally {
			this.busy = '';
		}
	}

	/** Which portrait a member has: 0 for none. */
	portraitCount(member: string) {
		return this.view?.project.cast?.find((m) => m.id === member)?.portrait ?? 0;
	}

	/** The portrait of a member, once it is here. Asking for it fetches it. */
	portrait(member: string): string | undefined {
		const n = this.portraitCount(member);
		if (!n) return undefined;
		const key = `${this.id}:${member}:${n}`;
		if (!this.#asked.has(key)) {
			this.#asked.add(key);
			const film = this.id;
			api.studioPortrait(film, member)
				.then((blob) => (this.portraits = { ...this.portraits, [key]: URL.createObjectURL(blob) }))
				.catch(() => this.#asked.delete(key));
		}
		return this.portraits[key];
	}

	// --- what is said: lines, the dub, subtitles ---------------------------------------------------

	addLine(shot: string, who = '') {
		const id = newId();
		this.edit('Add line', (doc) => doc.shots.find((s) => s.id === shot)?.lines.push({ id, who, text: '', said: {} }));
		return id;
	}

	setLine(shot: string, line: string, label: string, change: (l: DocLine) => void, key = '') {
		this.edit(
			label,
			(doc) => {
				const l = doc.shots.find((s) => s.id === shot)?.lines.find((x) => x.id === line);
				if (l) change(l);
			},
			key ? `${key}:${line}` : ''
		);
	}

	removeLine(shot: string, line: string) {
		this.edit('Delete line', (doc) => {
			const s = doc.shots.find((x) => x.id === shot);
			if (s) s.lines = s.lines.filter((l) => l.id !== line);
		});
	}

	/** The language that is heard and read. Its voice track is on, the others are muted. */
	showIn(language: string) {
		if (!this.doc) return;
		const own = language.toLowerCase() === this.ownLanguage.toLowerCase();
		this.edit('Language', (doc) => {
			doc.dub = own ? '' : language;
			const heard = doc.tracks.some((t) => t.kind === 'voice' && t.lang.toLowerCase() === language.toLowerCase()) ? language : this.ownLanguage;
			if (!doc.tracks.some((t) => t.kind === 'voice' && t.lang.toLowerCase() === heard.toLowerCase())) return;
			for (const t of doc.tracks) if (t.kind === 'voice') t.mute = t.lang.toLowerCase() !== heard.toLowerCase();
		});
	}

	/**
	 * Work the service does on the film itself and reports on: started, looked at until it is
	 * done, and then the film is taken as the service has it. One step for undo.
	 */
	async #job(kind: 'import' | 'dub' | 'listen', label: string, start: () => Promise<StudioJob>): Promise<StudioJob | null> {
		if (!this.doc || this.jobs[kind]?.state === 'working') return null;
		const id = this.id;
		this.problem = '';
		let held = false;
		try {
			await this.saveNow();
			const before = $state.snapshot(this.doc) as Doc;
			let job = await start();
			// From here the service changes the film: an edit made meanwhile waits, and is put together with its work.
			this.#held++;
			held = true;
			this.jobs = { ...this.jobs, [kind]: job };
			while (job.state === 'working') {
				await new Promise((r) => setTimeout(r, 1200));
				job = await api.studioJob(id, kind);
				if (this.id === id) this.jobs = { ...this.jobs, [kind]: job };
			}
			if (this.id !== id) return job;
			if (job.state === 'failed') {
				this.problem = say(job.error ?? '');
				return job;
			}
			const view = await api.studioRead(id);
			if (this.id !== id || !this.doc) return job;
			const theirs = docOf(view.project);
			const edited = this.#dirty;
			this.history.push(before, label);
			this.view = view;
			this.doc = edited ? takeWork(kind, $state.snapshot(this.doc) as Doc, theirs) : theirs;
			this.edits++;
			this.jumps++;
			this.loadSounds();
			return job;
		} catch (e) {
			this.problem = say(e);
			this.jobs = { ...this.jobs, [kind]: { state: 'none' } };
			return null;
		} finally {
			if (held) this.#held--;
			// What was edited while the service worked is saved now.
			if (this.#dirty && this.id === id) this.#touch();
		}
	}

	/** The voice reads every line onto a voice track; another language is translated first. */
	async dub(language?: string) {
		const job = await this.#job('dub', 'Dub', () => api.studioDub(this.id, language));
		if (job?.state === 'done') {
			app.track('studio_dubbed', { lines: job.lines ?? 0, translated: Boolean(language), spoken: job.spoken !== false });
			this.note = job.spoken === false ? `Translated into ${job.language}. The voice does not read it: subtitles only.` : `Dubbed: ${job.lines} ${job.lines === 1 ? 'line' : 'lines'}.`;
		}
	}

	/** The listening model writes down what is said in the clips that have no lines. */
	async listen(shots?: string[]) {
		const job = await this.#job('listen', 'Write down what is said', () => api.studioListen(this.id, shots));
		if (job?.state === 'done') {
			app.track('studio_listened', { lines: job.lines ?? 0 });
			this.note = job.lines ? `Wrote down ${job.lines} ${job.lines === 1 ? 'line' : 'lines'}.` : 'Nothing was said in these clips.';
		}
	}

	/** Brings a video file into the film as shots, cut where its scenes change. */
	async importVideo(path: string) {
		if (!path.trim()) return false;
		const job = await this.#job('import', 'Bring in a video', () => api.studioImport(this.id, path.trim()));
		if (job?.state === 'done') {
			app.track('studio_video_imported', { shots: job.shots ?? 0, left_out: Boolean(job.left_out) });
			this.note = `${job.shots} ${job.shots === 1 ? 'shot' : 'shots'} brought in.${job.left_out ? ' The film is full: the end of the video was left out.' : ''}`;
			this.fits++;
		}
		return job?.state === 'done';
	}

	/**
	 * A film from a few plain words. The chat model directs it (a brief: title, shape, length,
	 * look, cast, speech), the film is started in a folder of its own, the shots are written for
	 * it and making them starts. `stage` is told where this is. What was chosen stays changeable.
	 */
	async direct(text: string, wish: Partial<StudioBrief> & { folder?: string; asWritten?: boolean; voice?: number | null; subtitles?: boolean } = {}, stage: (at: 'directing' | 'writing' | 'starting') => void = () => {}) {
		this.problem = '';
		let opened: StudioView | null = null;
		try {
			stage('directing');
			// A film whose shots are used as written needs no director: what was set is the brief.
			const chosen: Partial<StudioBrief> = wish.asWritten ? {} : await api.studioBrief(text);
			const brief = { ...chosen, ...Object.fromEntries(Object.entries(wish).filter(([, v]) => v !== undefined && v !== '')) } as Partial<StudioBrief>;
			opened = await api.studioOpen(wish.folder?.trim() ?? '', brief.title || text.split(/\s+/).slice(0, 6).join(' '));
			const id = opened.project.id;
			// A folder that already holds a film is opened as it is.
			if (opened.project.shots.length > 0) {
				this.adopt(opened);
				return true;
			}
			const frames = brief.frames ?? this.usualFrames;
			const count = wish.shots ?? brief.shots ?? Math.max(1, Math.round((brief.seconds ?? 30) / (frames / 24)));
			await api.studioSave(id, {
				size: brief.size,
				look: brief.look ?? '',
				cast: (brief.cast ?? []).map((m) => ({ name: m.name, look: m.look })),
				speech: Boolean(brief.speech),
				language: brief.language ?? '',
				voice: wish.voice ?? null,
				subtitles: { show: wish.subtitles ?? true, burn: false },
				auto_export: true
			});
			stage('writing');
			const shots = await api.studioStoryboard(id, text, count, Boolean(wish.asWritten));
			stage('starting');
			const carries = Boolean(opened.can_continue);
			await api.studioSave(id, {
				shots: shots.map((s, i) => ({ prompt: s.prompt, frames, continues: carries && i > 0 && Boolean(s.continues), lines: (s.lines ?? []).map((l) => ({ who: l.who, text: l.text })) }))
			});
			this.adopt(await api.studioRender(id));
			// A film that was not directed has only the first words as its name: the chat model
			// writes one when it is in memory anyway. Making the film does not wait for it.
			if (!brief.title) void this.#name(id, text, opened.project.name);
			app.track('studio_quick_film', { seconds: brief.seconds ?? 0, shots: shots.length, shape: brief.shape ?? '', own_folder: Boolean(wish.folder?.trim()), cast: brief.cast?.length ?? 0, speech: Boolean(brief.speech), detailed: Object.keys(wish).length > 0 });
			return true;
		} catch (e) {
			// The folder is there: the film is opened, and the idea can be tried again from its Inspector.
			if (opened) this.adopt(opened);
			this.problem = say(e);
			return false;
		} finally {
			this.refresh();
		}
	}

	async #name(id: string, text: string, was: string) {
		try {
			const named = await api.title({ text, load: false });
			// Only while the film is open and still has the name it started with.
			if (named?.by === 'model' && this.id === id && this.doc?.name === was) this.edit('Name of the film', (d) => (d.name = named.title), 'film-name');
		} catch {
			// the first words stay
		}
	}

	/** A film that starts from a video file: a folder named after the file, and the file cut into its shots. */
	async fromVideo(path: string, folder = '') {
		this.problem = '';
		try {
			const name = (tail(path).replace(/\.[^.]+$/, '') || 'Film').slice(0, 60);
			this.adopt(await api.studioOpen(folder.trim(), name));
			this.refresh();
			return await this.importVideo(path);
		} catch (e) {
			this.problem = say(e);
			return false;
		}
	}

	// --- the library of sounds --------------------------------------------------------------------

	async loadSounds() {
		const id = this.id;
		try {
			const sounds = await api.studioSounds(id);
			if (this.id === id) this.sounds = sounds;
		} catch {
			// The list stays as it was.
		}
	}

	/** Adds a sound file by its full path, or every sound file in a folder. Answers with what was added. */
	async addSound(path: string): Promise<string[]> {
		if (!path.trim() || this.busy) return [];
		this.busy = 'adding';
		this.problem = '';
		try {
			const answer = await api.studioAddSound(this.id, path.trim());
			this.sounds = answer.sounds;
			app.track('studio_sound_added', { files: answer.added.length });
			return answer.added;
		} catch (e) {
			this.problem = say(e);
			return [];
		} finally {
			this.busy = '';
		}
	}

	async removeSound(file: string) {
		try {
			this.sounds = await api.studioRemoveSound(this.id, file);
		} catch (e) {
			this.problem = say(e);
		}
	}

	/** Reads a sound file for playing. One that cannot be read or played is named once. */
	loadSound(file: string) {
		const id = this.id;
		return this.audio.load(file, () => api.studioSoundFile(id, file)).then((buffer) => {
			if (!buffer && this.id === id && !this.silent.includes(tail(file))) this.silent = [...this.silent, tail(file)];
			return buffer;
		});
	}

	async preview(file: string, done: () => void) {
		const id = this.id;
		return this.audio.preview(file, () => api.studioSoundFile(id, file), done);
	}

	// --- pictures and waveforms -------------------------------------------------------------------

	/** At most two of these run at once: a long film asks for many. */
	#later(job: () => Promise<void>) {
		this.#queue.push(job);
		const next = () => {
			const run = this.#queue.shift();
			if (!run) return;
			this.#working++;
			run().finally(() => {
				this.#working--;
				next();
			});
		};
		if (this.#working < 2) next();
	}

	posterKey(s: StudioShot) {
		return `${this.id}:${s.id}:${s.take ?? 0}:${s.seconds ?? 0}`;
	}

	peaksKey(s: StudioShot) {
		return `shot:${this.id}:${s.id}:${s.take ?? 0}:${s.seconds ?? 0}`;
	}

	/** Fetches what the lanes draw and what plays: called whenever the film or its clips change. */
	gather() {
		if (!this.doc) return;
		const film = this.id;
		for (const s of this.shots) {
			if (!hasClip(s)) continue;
			const key = this.posterKey(s);
			if (!this.#asked.has(key)) {
				this.#asked.add(key);
				this.#later(async () => {
					try {
						const image = new Image();
						image.src = URL.createObjectURL(await api.studioPoster(film, s.id));
						await image.decode();
						this.posters.set(key, image);
						this.drawn++;
					} catch {
						this.#asked.delete(key);
					}
				});
			}
			const wave = this.peaksKey(s);
			if (!this.#asked.has(wave)) {
				this.#asked.add(wave);
				this.#later(() => this.#peaks(wave, film, { shot: s.id }));
			}
			for (const t of s.takes ?? []) {
				const take = `${film}:${s.id}.${t.n}`;
				if (this.#asked.has(take)) continue;
				this.#asked.add(take);
				this.#later(async () => {
					try {
						const url = URL.createObjectURL(await api.studioPoster(film, s.id, t.n));
						this.takeUrls = { ...this.takeUrls, [`${s.id}.${t.n}`]: url };
					} catch {
						this.#asked.delete(take);
					}
				});
			}
		}
		for (const t of this.doc.tracks) {
			for (const r of t.regions) {
				this.peaksOf(`file:${r.file}`);
				this.loadSound(r.file);
			}
		}
	}

	async #peaks(key: string, film: string, of: { shot: string } | { file: string }) {
		try {
			const p = await api.studioPeaks(film, of);
			this.peaks.set(key, { rate: p.rate, seconds: p.seconds, pairs: Int8Array.from(p.peaks) });
			this.drawn++;
		} catch {
			// No waveform: the region is drawn without one.
			this.peaks.set(key, { rate: 50, seconds: 0, pairs: new Int8Array(0) });
		}
	}

	#waiting = new Map<string, Promise<Peaks | undefined>>();
	/** The peaks of a sound file ("file:/path"), fetched once. */
	peaksOf(key: string): Promise<Peaks | undefined> {
		const have = this.peaks.get(key);
		if (have) return Promise.resolve(have);
		let wait = this.#waiting.get(key);
		if (!wait) {
			const film = this.id;
			wait = new Promise((done) => this.#later(() => this.#peaks(key, film, { file: key.slice(5) }).then(() => done(this.peaks.get(key)))));
			this.#waiting.set(key, wait);
		}
		return wait;
	}

	/** The clip of a shot as something a video element plays. The last few are kept: a long film has many. */
	clipOf(shot: string, take?: number): Promise<string | null> {
		const s = this.shot(shot);
		if (!s) return Promise.resolve(null);
		const key = `${this.id}:${shot}:${take ?? s.take ?? 0}:${take ? 0 : (s.seconds ?? 0)}`;
		let url = this.#clips.get(key);
		this.#clips.delete(key);
		url ??= api
			.studioClip(this.id, shot, take)
			.then((blob) => URL.createObjectURL(blob))
			.catch(() => null);
		this.#clips.set(key, url);
		if (this.#clips.size > 14) {
			const [oldest, old] = this.#clips.entries().next().value!;
			this.#clips.delete(oldest);
			old.then((u) => u && URL.revokeObjectURL(u));
		}
		return url;
	}

	// --- the transport ----------------------------------------------------------------------------

	/** Called with the playhead's moment whenever it moves: every frame while the film plays. */
	onFrame(listener: (time: number) => void) {
		this.#listeners.add(listener);
		return () => this.#listeners.delete(listener);
	}

	#tell() {
		for (const listener of this.#listeners) listener(this.time);
	}

	/** Where playing ends: the end of the pictures, or of the last sound in a film without any. */
	get end() {
		return this.plan.total > 0 ? this.plan.total : this.length;
	}

	#regions(): PlayRegion[] {
		return (this.doc?.tracks ?? []).flatMap((t) => t.regions.map((r) => ({ ...r, track: t.id })));
	}

	/** The mix as it is now, to the sound engine. */
	mix() {
		if (!this.doc) return;
		this.audio.setMix(
			this.strips.map((s) => ({ id: s.id, gain: stripGain(s.volume, this.heard[s.id]), pan: s.pan })),
			this.doc.master
		);
	}

	/** The regions changed while the film plays: they are set again from where the playhead is. */
	replay() {
		if (this.playing) this.audio.start(this.time, this.#regions());
	}

	seek(seconds: number) {
		this.time = Math.min(Math.max(0, seconds), Math.max(this.length, this.end));
		if (this.playing) this.audio.start(this.time, this.#regions());
		else this.jumps++;
		this.#tell();
	}

	step(frames: number) {
		this.seek(snapFrame(this.time) + frames / FPS);
	}

	play() {
		if (!this.doc || this.playing || this.end <= 0) return;
		this.audio.resume();
		this.mix();
		const cycling = this.cycle.on && this.cycle.to > this.cycle.from;
		if (cycling && (this.time < this.cycle.from || this.time >= this.cycle.to - 0.02)) this.time = this.cycle.from;
		else if (this.time >= this.end - 0.02) this.time = 0;
		this.playing = true;
		this.audio.start(this.time, this.#regions());
		// Pictures are not drawn while the window is hidden; a slow beat keeps the film going.
		this.#beat = setInterval(() => this.#advance(), 200);
		this.#frame = requestAnimationFrame(this.#run);
		app.track('studio_played', { shots: this.exportPlan.parts.length, tracks: this.doc.tracks.length });
	}

	pause() {
		if (!this.playing) return;
		this.playing = false;
		this.time = this.audio.time();
		this.audio.stop();
		cancelAnimationFrame(this.#frame);
		clearInterval(this.#beat);
		this.jumps++;
		this.#tell();
	}

	togglePlay() {
		if (this.playing) this.pause();
		else this.play();
	}

	/** Stop: the film stands; pressed again, the playhead goes back to the start (or the start of the cycle). */
	stop() {
		if (this.playing) this.pause();
		else this.seek(this.cycle.on ? this.cycle.from : 0);
	}

	#run = () => {
		if (!this.playing) return;
		this.#advance();
		if (this.playing) this.#frame = requestAnimationFrame(this.#run);
	};

	#advance() {
		if (!this.playing) return;
		let t = this.audio.time();
		const cycling = this.cycle.on && this.cycle.to > this.cycle.from;
		const end = cycling ? Math.min(this.cycle.to, this.end) : this.end;
		if (t >= end) {
			if (!cycling) {
				this.pause();
				this.time = end;
				this.jumps++;
				this.#tell();
				return;
			}
			t = this.cycle.from;
			this.time = t;
			this.audio.start(t, this.#regions());
		}
		this.time = t;
		this.#tell();
	}

	toggleCycle() {
		// Without a range yet: the selected shots or regions, else the whole film.
		if (!this.cycle.on && this.cycle.to <= this.cycle.from) {
			const parts = this.plan.parts.filter((p) => this.shotsSel.includes(p.shot));
			const regions = this.regionsSel.map((id) => this.region(id)?.region).filter((r) => r !== undefined);
			if (parts.length > 0) this.cycle = { on: true, from: Math.min(...parts.map((p) => p.start)), to: Math.max(...parts.map((p) => p.start + p.length)) };
			else if (regions.length > 0) this.cycle = { on: true, from: Math.min(...regions.map((r) => r.start)), to: Math.max(...regions.map(regionEnd)) };
			else this.cycle = { on: true, from: 0, to: this.end };
			return;
		}
		this.cycle.on = !this.cycle.on;
	}

	// --- zoom and panels --------------------------------------------------------------------------

	zoom(factor: number) {
		this.pps = Math.min(MAX_PPS, Math.max(MIN_PPS, this.pps * factor));
	}

	set<K extends keyof Layout>(key: K, value: Layout[K]) {
		this.layout[key] = value;
		remembered.setJson('studio.layout', $state.snapshot(this.layout));
	}

	/** Shows a tab of the bottom panel; the same key again closes the panel. */
	bottom(tab: 'mixer' | 'editor' | 'cast') {
		if (this.layout.bottom && this.layout.tab === tab) this.set('bottom', false);
		else {
			this.set('tab', tab);
			this.set('bottom', true);
		}
	}

	/** The length of a shot's clip in seconds, for trims. */
	clipSeconds(id: string) {
		const s = this.shot(id);
		return s ? shotSeconds(s) : 0;
	}
}

export const studio = new Studio();
