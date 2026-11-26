// The sound of the Studio workstation: every sound region through its own gain into the strip
// of its track, every strip (volume, mute, solo, pan as balance) into the master, and meters on
// all of them. The shots' own sound is read from their clips and played the same way, each shot
// through a level the viewer sets (its volume, its fades, the ducking), on one more strip. The
// video elements are silent: a media element routed into the mixer makes WebKit show its
// pictures in fits and starts. It is also the clock the film plays by.

import { balance, fadeLevel, regionSpan } from './studio-edit.ts';

/** A strip as it is mixed: its gain is nothing when it is muted or another strip is soloed. */
export interface StripMix {
	id: string;
	gain: number;
	pan: number;
}

/** A region as it is played. */
export interface PlayRegion {
	track: string;
	file: string;
	start: number;
	offset: number;
	length: number;
	loops: number;
	gain: number;
	fade_in: number;
	fade_out: number;
	/** A shot's own sound: the shot whose level (see `level`) it is heard through. */
	shot?: string;
}

/** How many shots' sounds are kept read: a long film has many, and each is some megabytes. */
const KEPT_CLIPS = 16;

interface Strip {
	input: GainNode;
	left: GainNode;
	right: GainNode;
	meters: [AnalyserNode, AnalyserNode];
}

/** The strip of the shots' own sound. */
export const SHOTS = 'shots';
export const MASTER = 'master';

export class StudioAudio {
	ctx: AudioContext | null = null;
	#master: GainNode | null = null;
	#masterMeters: [AnalyserNode, AnalyserNode] | null = null;
	#strips = new Map<string, Strip>();
	#mix: StripMix[] = [];
	#masterGain = 1;
	#buffers = new Map<string, Promise<AudioBuffer | null>>();
	#ready = new Map<string, AudioBuffer>();
	#live: { src: AudioBufferSourceNode; gain: GainNode }[] = [];
	/** The level of each shot's own sound, set by the viewer, and the value it was last set to. */
	#levels = new Map<string, { node: GainNode; value: number }>();
	/** What the clock plays now: a sound that is read late joins in if it is among them. */
	#wanted: PlayRegion[] = [];
	#scratch = new Float32Array(1024);
	#preview: AudioBufferSourceNode | null = null;
	/** Changes with every start and stop, so a file that is ready late knows whether it is still wanted. */
	#run = 0;
	/** The moment of the film the clock was started at, and the context's time then. */
	#from = 0;
	#since = 0;
	running = false;

	/** The context is made at the first press of Play: a browser lets sound start only then. */
	ensure(): AudioContext {
		if (this.ctx) return this.ctx;
		const ctx = new AudioContext({ latencyHint: 'interactive' });
		this.ctx = ctx;
		this.#master = ctx.createGain();
		this.#master.gain.value = this.#masterGain;
		this.#master.connect(ctx.destination);
		this.#masterMeters = this.#meters(this.#master);
		this.#apply();
		return ctx;
	}

	resume() {
		const ctx = this.ensure();
		if (ctx.state === 'suspended') ctx.resume().catch(() => {});
	}

	/** Two meters, left and right, on what comes out of a node. */
	#meters(node: AudioNode): [AnalyserNode, AnalyserNode] {
		const ctx = this.ctx!;
		const split = ctx.createChannelSplitter(2);
		node.connect(split);
		const pair = [0, 1].map((side) => {
			const meter = ctx.createAnalyser();
			meter.fftSize = 1024;
			split.connect(meter, side);
			return meter;
		});
		return pair as [AnalyserNode, AnalyserNode];
	}

	#strip(id: string): Strip {
		let strip = this.#strips.get(id);
		if (strip) return strip;
		const ctx = this.ensure();
		// One channel in is both sides; the balance then turns one of them down.
		const input = ctx.createGain();
		input.channelCount = 2;
		input.channelCountMode = 'explicit';
		input.channelInterpretation = 'speakers';
		const split = ctx.createChannelSplitter(2);
		const merge = ctx.createChannelMerger(2);
		const left = ctx.createGain();
		const right = ctx.createGain();
		input.connect(split);
		split.connect(left, 0);
		split.connect(right, 1);
		left.connect(merge, 0, 0);
		right.connect(merge, 0, 1);
		merge.connect(this.#master!);
		strip = { input, left, right, meters: this.#meters(merge) };
		this.#strips.set(id, strip);
		return strip;
	}

	/** The mix as the film has it now. Changes are heard at once, without a click. */
	setMix(strips: StripMix[], master: number) {
		this.#mix = strips;
		this.#masterGain = master;
		if (this.ctx) this.#apply();
	}

	#apply() {
		const ctx = this.ctx!;
		const now = ctx.currentTime;
		this.#master!.gain.setTargetAtTime(this.#masterGain, now, 0.012);
		for (const s of this.#mix) {
			const strip = this.#strip(s.id);
			const [l, r] = balance(s.pan);
			strip.input.gain.setTargetAtTime(s.gain, now, 0.012);
			strip.left.gain.setTargetAtTime(l, now, 0.012);
			strip.right.gain.setTargetAtTime(r, now, 0.012);
		}
		// A track that is gone is silent.
		for (const [id, strip] of this.#strips) if (!this.#mix.some((s) => s.id === id)) strip.input.gain.setTargetAtTime(0, now, 0.012);
	}

	/** The level a shot's own sound is heard at: its volume, its fade and the ducking, from the viewer. */
	level(shot: string, value: number) {
		if (!this.ctx) return;
		const have = this.#levels.get(shot);
		if (have && Math.abs(have.value - value) < 0.0005) return;
		const node = have?.node ?? this.#level(shot);
		node.gain.setTargetAtTime(value, this.ctx.currentTime, 0.006);
		this.#levels.set(shot, { node, value });
	}

	#level(shot: string): GainNode {
		let have = this.#levels.get(shot);
		if (!have) {
			const node = this.ensure().createGain();
			node.connect(this.#strip(SHOTS).input);
			have = { node, value: 1 };
			this.#levels.set(shot, have);
		}
		return have.node;
	}

	/** Reads a sound file once and keeps it decoded. `null` when it cannot be read or played. */
	load(file: string, read: () => Promise<ArrayBuffer>): Promise<AudioBuffer | null> {
		let buffer = this.#buffers.get(file);
		if (!buffer) {
			buffer = read()
				.then((bytes) => this.ensure().decodeAudioData(bytes))
				.then((decoded) => {
					this.#ready.set(file, decoded);
					return decoded;
				})
				.catch(() => null);
			this.#buffers.set(file, buffer);
			// Asked for while the film plays: it joins in where it is wanted.
			const run = this.#run;
			for (const r of this.#wanted) if (r.file === file) buffer.then((late) => late && run === this.#run && this.running && this.#play(r, late));
			if (file.startsWith('clip:')) this.#keep(file);
		}
		return buffer;
	}

	/** Only the clips' sounds read last are kept. */
	#keep(file: string) {
		const clips = [...this.#buffers.keys()].filter((k) => k.startsWith('clip:') && k !== file);
		for (const old of clips.slice(0, Math.max(0, clips.length - KEPT_CLIPS + 1))) this.forget(old);
	}

	/** Whether a file was asked for and could not be read. */
	async failed(file: string) {
		const buffer = this.#buffers.get(file);
		return buffer ? (await buffer) === null : false;
	}

	forget(file: string) {
		this.#buffers.delete(file);
		this.#ready.delete(file);
	}

	/**
	 * The context's moment that is heard at a moment of `performance.now()`: what the speakers play
	 * then, which is later than what the context works on by the output's latency.
	 */
	#heard(perf: number): number {
		const ctx = this.ctx!;
		const stamp = ctx.getOutputTimestamp?.();
		if (stamp?.performanceTime && stamp.contextTime !== undefined) return stamp.contextTime + (perf - stamp.performanceTime) / 1000;
		const late = (ctx.baseLatency ?? 0) + ((ctx as AudioContext & { outputLatency?: number }).outputLatency ?? 0);
		return ctx.currentTime - late + (perf - performance.now()) / 1000;
	}

	/** The moment of the film that is heard at a moment of `performance.now()`. */
	heardAt(perf: number): number {
		if (!this.running || !this.ctx) return this.#from;
		return this.#from + Math.max(0, this.#heard(perf) - this.#since);
	}

	/**
	 * Starts the clock at a moment of the film and every region that is still to be heard from
	 * there. With `shownAt` (a moment of `performance.now()`) the film's moment `at` is heard
	 * then: the sound joins pictures that are already on the screen.
	 */
	start(at: number, regions: PlayRegion[], shownAt?: number) {
		const ctx = this.ensure();
		this.#silence();
		const run = ++this.#run;
		this.#from = at;
		this.#since = shownAt === undefined ? ctx.currentTime + 0.05 : this.#heard(shownAt);
		this.running = true;
		this.#schedule(regions, run);
	}

	#schedule(regions: PlayRegion[], run: number) {
		this.#wanted = regions;
		for (const r of regions) {
			const buffer = this.#ready.get(r.file);
			if (buffer) this.#play(r, buffer);
			// A file that is still being read joins in when it is there.
			else this.#buffers.get(r.file)?.then((late) => late && run === this.#run && this.running && this.#play(r, late));
		}
	}

	/** The regions changed while the clock runs: they are set again from now, on the same clock. */
	reschedule(regions: PlayRegion[]) {
		if (!this.running || !this.ctx) return;
		this.#silence();
		this.#schedule(regions, ++this.#run);
	}

	#play(r: PlayRegion, buffer: AudioBuffer) {
		const ctx = this.ctx!;
		const span = regionSpan(r);
		const film = this.time();
		const into = Math.max(0, film - r.start);
		if (into >= span - 0.005 || r.offset >= buffer.duration) return;
		// The context's time at a moment of the region.
		const when = (at: number) => Math.max(ctx.currentTime, this.#since + (r.start + at - this.#from));
		const src = ctx.createBufferSource();
		src.buffer = buffer;
		const gain = ctx.createGain();
		const level = gain.gain;
		level.setValueAtTime(r.gain * fadeLevel(r, into), when(into));
		if (r.fade_in > 0 && into < r.fade_in) level.linearRampToValueAtTime(r.gain, when(r.fade_in));
		if (r.fade_out > 0) {
			const down = span - r.fade_out;
			if (into < down) level.setValueAtTime(r.gain, when(down));
			level.linearRampToValueAtTime(0, when(span));
		}
		src.connect(gain);
		gain.connect(r.shot ? this.#level(r.shot) : this.#strip(r.track).input);
		if (r.loops > 1) {
			src.loop = true;
			src.loopStart = r.offset;
			src.loopEnd = Math.min(buffer.duration, r.offset + r.length);
		}
		src.start(when(into), r.offset + (into % r.length), span - into);
		const entry = { src, gain };
		src.onended = () => {
			gain.disconnect();
			this.#live = this.#live.filter((x) => x !== entry);
		};
		this.#live.push(entry);
	}

	#silence() {
		for (const { src, gain } of this.#live) {
			src.onended = null;
			try {
				src.stop();
			} catch {
				// it had not started
			}
			gain.disconnect();
		}
		this.#live = [];
	}

	/** Stops the clock where it is and every region with it. */
	stop() {
		if (!this.running) return;
		this.#from = this.time();
		this.running = false;
		this.#run++;
		this.#silence();
	}

	/** Stops the clock at a moment of the film: nothing is heard until it is started again. */
	hold(at: number) {
		this.stop();
		this.#from = at;
	}

	/** The moment of the film the clock is at. */
	time(): number {
		if (!this.running || !this.ctx) return this.#from;
		return this.#from + Math.max(0, this.ctx.currentTime - this.#since);
	}

	/** The highest level on each side of a strip (or of the master) right now: 1 is full level. */
	levels(id: string): [number, number] {
		const meters = id === MASTER ? this.#masterMeters : this.#strips.get(id)?.meters;
		if (!meters || this.ctx?.state !== 'running') return [0, 0];
		return meters.map((meter) => {
			meter.getFloatTimeDomainData(this.#scratch);
			let peak = 0;
			for (let i = 0; i < this.#scratch.length; i++) {
				const v = Math.abs(this.#scratch[i]);
				if (v > peak) peak = v;
			}
			return peak;
		}) as [number, number];
	}

	/** Plays a file by itself, from its start, to hear what it is. Again stops it. */
	async preview(file: string, read: () => Promise<ArrayBuffer>, done: () => void = () => {}) {
		const was = this.#preview;
		this.stopPreview();
		if (was && (was as AudioBufferSourceNode & { file?: string }).file === file) return false;
		this.resume();
		const buffer = await this.load(file, read);
		if (!buffer || !this.ctx) return false;
		const src: AudioBufferSourceNode & { file?: string } = this.ctx.createBufferSource();
		src.buffer = buffer;
		src.file = file;
		src.connect(this.#master!);
		src.onended = () => {
			if (this.#preview === src) this.#preview = null;
			done();
		};
		src.start();
		this.#preview = src;
		return true;
	}

	stopPreview() {
		const src = this.#preview;
		this.#preview = null;
		try {
			src?.stop();
		} catch {
			// already over
		}
	}

	close() {
		this.stop();
		this.stopPreview();
		this.ctx?.close().catch(() => {});
		this.ctx = null;
		this.#strips.clear();
		this.#levels.clear();
		this.#buffers.clear();
		this.#ready.clear();
	}
}
