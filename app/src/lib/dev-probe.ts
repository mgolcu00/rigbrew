// Development only: measures how the Studio viewer plays, in the app's own webview, for a build
// made with VITE_RIGBREW_PROBE=http://127.0.0.1:<port>. It asks a small server on this computer
// for what to do (a dev view, press play, measure) and hands back what it measured. A build
// without that variable has none of this.

import { MASTER, SHOTS, studio } from './studio.svelte.ts';

type View = (view: Record<string, string>) => void;

interface Command {
	view?: Record<string, string>;
	do?: 'play' | 'playFilm' | 'pause' | 'start' | 'dock' | 'fit';
	measure?: { seconds?: number; label?: string };
	/** Pixels a second on the timeline: a zoomed-in timeline scrolls while the film plays. */
	pps?: number;
	/** The playhead to a moment of the film, in seconds, playing or not. */
	seek?: number;
	/** Panels of the Studio, as `studio.set` takes them. */
	layout?: Record<string, unknown>;
	/** The pointer comes to rest on the middle of the first element that matches, as for a help tag. */
	hover?: string;
	/** The list of keys and tools, opened, with these words typed into its search. */
	keys?: string;
	/** Clicks the first element that matches. */
	click?: string;
	/** Reads the own sound of the first made shot as the mixer does, and says what came of it. */
	sound?: boolean;
}

const sleep = (ms: number) => new Promise((done) => setTimeout(done, ms));

export function startProbe(base: string, view: View) {
	const send = (report: unknown) => fetch(`${base}/report`, { method: 'POST', body: JSON.stringify(report) }).catch(() => {});
	const poll = async () => {
		try {
			const cmd = (await (await fetch(`${base}/next`, { cache: 'no-store' })).json()) as Command;
			if (cmd.view) view(cmd.view);
			if (cmd.do === 'play') studio.play();
			else if (cmd.do === 'playFilm') studio.playFilm();
			else if (cmd.do === 'pause') studio.pause();
			else if (cmd.do === 'start') studio.seek(0);
			else if (cmd.do === 'dock') studio.viewerLarge = false;
			else if (cmd.do === 'fit') studio.fits++;
			if (cmd.pps) studio.pps = cmd.pps;
			if (cmd.seek !== undefined) studio.seek(cmd.seek);
			for (const [key, value] of Object.entries(cmd.layout ?? {})) studio.set(key as 'tab', value as 'mixer');
			if (cmd.hover !== undefined) {
				// The pointer at the middle of the element: what is under it there gets the event, as with a real
				// pointer (a disabled control lets it through to what is behind).
				const box = cmd.hover ? document.querySelector(cmd.hover)?.getBoundingClientRect() : undefined;
				const [x, y] = box ? [box.left + box.width / 2, box.top + box.height / 2] : [0, 0];
				const el = box ? document.elementFromPoint(x, y) : document.body;
				el?.dispatchEvent(new PointerEvent('pointerover', { bubbles: true, pointerType: 'mouse', clientX: x, clientY: y }));
			}
			if (cmd.click) document.querySelector<HTMLElement>(cmd.click)?.click();
			if (cmd.sound) {
				const s = studio.shots.find((x) => x.state === 'done');
				try {
					const bytes = (s ? await studio.clipBytes(s.id) : null) ?? new ArrayBuffer(0);
					const read = await studio.audio.ensure().decodeAudioData(bytes);
					send({ sound: { shot: s?.id, bytes: bytes.byteLength, seconds: read.duration, rate: read.sampleRate } });
				} catch (e) {
					send({ sound: String(e) });
				}
			}
			if (cmd.keys !== undefined) {
				studio.keysOpen = true;
				await sleep(100);
				const find = document.querySelector<HTMLInputElement>('.keys .find');
				if (find) {
					find.value = cmd.keys;
					find.dispatchEvent(new Event('input', { bubbles: true }));
				}
			}
			if (cmd.measure) send(await measure(cmd.measure.seconds ?? 20, cmd.measure.label ?? ''));
			else if (Object.keys(cmd).length > 0) send({ done: cmd, width: innerWidth });
		} catch (e) {
			send({ error: String(e) });
		}
		setTimeout(poll, 500);
	};
	send({ hello: true, width: innerWidth, dpr: devicePixelRatio, agent: navigator.userAgent });
	poll();
}

interface Frame {
	at: number;
	el: number;
	media: number;
	shown: boolean;
	offset: number | null;
	output: number | null;
}

/** Watches the viewer for a while: the frames each video element presented, the main thread, the picture against the sound. */
async function measure(seconds: number, label: string) {
	const videos = [...document.querySelectorAll<HTMLVideoElement>('.viewer video')];
	const quality0 = videos.map((v) => v.getVideoPlaybackQuality?.());
	const t0 = performance.now();
	const frames: Frame[] = [];
	const seeks = videos.map(() => 0);
	const stalls = videos.map(() => 0);
	const ctx = studio.audio.ctx;
	const latency = (ctx?.baseLatency ?? 0) + ((ctx as AudioContext & { outputLatency?: number })?.outputLatency ?? 0);
	let alive = true;
	const seekTimes: string[] = [];
	const onSeek = videos.map((_, i) => () => {
		if (!alive) return;
		seeks[i]++;
		seekTimes.push(`${i}@${((performance.now() - t0) / 1000).toFixed(2)}`);
	});
	const onStall = videos.map((_, i) => () => alive && stalls[i]++);
	videos.forEach((v, i) => {
		v.addEventListener('seeking', onSeek[i]);
		v.addEventListener('waiting', onStall[i]);
		const each = (now: number, meta: VideoFrameCallbackMetadata) => {
			if (!alive) return;
			const shown = v.style.zIndex === '2' && Number(v.style.opacity || '0') > 0.5;
			// The picture against the sound: the film's moment of this frame, and the moment heard when it is shown.
			let offset: number | null = null;
			let output: number | null = null;
			if (shown && studio.playing && !studio.starting) {
				// As the sound is processed, less the latency the context reports; and as the output reports it is heard.
				const heard = studio.audio.time() + (meta.expectedDisplayTime - performance.now()) / 1000 - latency;
				const part = studio.plan.parts.find((p) => p.start <= heard && heard < p.start + p.length);
				if (part && heard - part.start > 0.15 && part.start + part.length - heard > 0.15) {
					const film = part.start + (meta.mediaTime - part.from);
					if (Math.abs(film - heard) < 1) offset = film - heard;
					const out = studio.audio.heardAt(meta.expectedDisplayTime);
					if (Math.abs(film - out) < 1) output = film - out;
				}
			}
			frames.push({ at: meta.expectedDisplayTime, el: i, media: meta.mediaTime, shown, offset, output });
			v.requestVideoFrameCallback(each);
		};
		v.requestVideoFrameCallback(each);
	});
	// The main thread: gaps between animation frames, and long tasks where the engine reports them.
	const gaps: number[] = [];
	const heard = { shots: 0, master: 0 };
	let last = performance.now();
	const tick = (now: number) => {
		if (!alive) return;
		gaps.push(now - last);
		// What the mixer hears of the shots' own sound and of everything, so a silent strip shows.
		heard.shots = Math.max(heard.shots, ...studio.audio.levels(SHOTS));
		heard.master = Math.max(heard.master, ...studio.audio.levels(MASTER));
		last = now;
		requestAnimationFrame(tick);
	};
	requestAnimationFrame(tick);
	const longTasks: number[] = [];
	const types = PerformanceObserver.supportedEntryTypes ?? [];
	let observer: PerformanceObserver | null = null;
	if (types.includes('longtask')) {
		observer = new PerformanceObserver((list) => list.getEntries().forEach((e) => longTasks.push(e.duration)));
		observer.observe({ type: 'longtask' });
	}
	const time0 = studio.time;
	await sleep(seconds * 1000);
	alive = false;
	observer?.disconnect();
	videos.forEach((v, i) => {
		v.removeEventListener('seeking', onSeek[i]);
		v.removeEventListener('waiting', onStall[i]);
	});
	const elapsed = (performance.now() - t0) / 1000;
	const quality1 = videos.map((v) => v.getVideoPlaybackQuality?.());
	const shown = frames.filter((f) => f.shown).sort((a, b) => a.at - b.at);
	const intervals = shown.slice(1).map((f, i) => ({ ms: f.at - shown[i].at, cut: f.el !== shown[i].el }));
	const frameMs = 1000 / 24;
	const offsets = shown.map((f) => f.offset).filter((o): o is number => o !== null);
	const sorted = [...offsets].sort((a, b) => a - b);
	const outputs = shown
		.map((f) => f.output)
		.filter((o): o is number => o !== null)
		.sort((a, b) => a - b);
	const pct = (list: number[], q: number) => (list.length ? Math.round(list[Math.min(list.length - 1, Math.floor(q * list.length))] * 1000) : null);
	const gapsSorted = [...gaps].sort((a, b) => a - b);
	return {
		label,
		width: innerWidth,
		height: innerHeight,
		dpr: devicePixelRatio,
		large: studio.viewerLarge,
		seconds: Math.round(elapsed * 100) / 100,
		filmSeconds: Math.round((studio.time - time0) * 100) / 100,
		presented: shown.length,
		fps: Math.round((shown.length / elapsed) * 10) / 10,
		allCallbacks: frames.length,
		hitches: intervals.filter((i) => i.ms > frameMs * 1.5).length,
		hitchesAtCuts: intervals.filter((i) => i.cut && i.ms > frameMs * 1.5).length,
		cuts: intervals.filter((i) => i.cut).length,
		worstGapMs: Math.round(Math.max(0, ...intervals.map((i) => i.ms))),
		worstCutGapMs: Math.round(Math.max(0, ...intervals.filter((i) => i.cut).map((i) => i.ms))),
		seeks,
		stalls,
		dropped: quality1.map((q, i) => (q && quality0[i] ? q.droppedVideoFrames - quality0[i]!.droppedVideoFrames : null)),
		decoded: quality1.map((q, i) => (q && quality0[i] ? q.totalVideoFrames - quality0[i]!.totalVideoFrames : null)),
		avMs: { n: offsets.length, p5: pct(sorted, 0.05), median: pct(sorted, 0.5), p95: pct(sorted, 0.95), latencyMs: Math.round(latency * 1000) },
		avOutputMs: { n: outputs.length, p5: pct(outputs, 0.05), median: pct(outputs, 0.5), p95: pct(outputs, 0.95) },
		rates: videos.map((v) => v.playbackRate),
		peakLevels: { shots: Math.round(heard.shots * 1000) / 1000, master: Math.round(heard.master * 1000) / 1000 },
		seekTimes,
		perElement: videos.map((_, i) => frames.filter((f) => f.el === i).length),
		// Where it went wrong: each long gap between shown frames (seconds into the measurement, its length, at a cut), and the picture against the sound every second.
		gapsAt: intervals.map((iv, k) => ({ ...iv, t: (shown[k + 1].at - t0) / 1000 })).filter((iv) => iv.ms > frameMs * 1.5).map((iv) => `${iv.t.toFixed(2)}:${Math.round(iv.ms)}${iv.cut ? 'c' : ''}`),
		avEachSecond: shown.filter((f, k) => f.offset !== null && (k === 0 || Math.floor((f.at - t0) / 1000) !== Math.floor((shown[k - 1].at - t0) / 1000))).map((f) => `${((f.at - t0) / 1000).toFixed(1)}:${Math.round(f.offset! * 1000)}`),
		raf: { frames: gaps.length, p50: Math.round(gapsSorted[Math.floor(gaps.length / 2)] ?? 0), over50: gaps.filter((g) => g > 50).length, over100: gaps.filter((g) => g > 100).length, worst: Math.round(Math.max(0, ...gaps)) },
		longTasks: types.includes('longtask') ? { n: longTasks.length, worst: Math.round(Math.max(0, ...longTasks)) } : 'not supported'
	};
}
