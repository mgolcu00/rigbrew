<script lang="ts">
	import { onDestroy } from 'svelte';
	import { cueAt, duckLevel } from '#lib/studio-edit.ts';
	import { STATE_WORD } from '#lib/studio-look.ts';
	import { tag } from '#lib/studio-tags.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { timecode } from '#lib/studio-time.ts';
	import { partLight, type FilmPart } from '#lib/studio.ts';
	import StudioIcon from './StudioIcon.svelte';

	// The picture of the film at the playhead, as the export will hold it: every shot from its trim
	// start to its trim end, dissolves as two pictures over each other, fades through black, the
	// film's own fades. Two video elements take turns: while one shows a shot, the other holds the
	// next one ready and already running, hidden, so the cut is a swap. They are
	// silent: the mixer plays each shot's own sound from its clip at its place (studio-audio.ts),
	// at the level set here, turned down under what is said as the export turns it down. The
	// subtitle of the moment lies over it.
	//
	// The sound is the clock. A playing picture is never sought to follow it: one that runs ahead
	// plays a little slower, one behind a little faster, measured on every frame it shows. It is
	// sought only when the playhead jumps, or when it is far off (a clip that came late). When the
	// film starts, the picture starts first and the sound joins at its first frame.

	const EDGE = 0.5; // seconds of the film's own fade in and fade out
	/** Farther off than this, a playing picture is sought; closer, its speed is nudged. */
	const FAR = 0.3;
	/**
	 * The next shot is set running this long before its cut, at a crawl, so that at the cut only its
	 * speed changes: starting a stopped element takes WebKit from a few to a hundred milliseconds.
	 */
	const PREROLL = 0.4;
	const CRAWL = 0.0625;
	/** Whether the webview says when each frame is shown; without it the picture is measured by its clock, less often. */
	const FRAMES = typeof HTMLVideoElement !== 'undefined' && 'requestVideoFrameCallback' in HTMLVideoElement.prototype;

	type Role = 'now' | 'out' | 'next';

	let videos = $state<(HTMLVideoElement | null)[]>([null, null]);
	/** The clip each element holds, and the part of the film it is for. */
	let held: string[] = ['', ''];
	let ready: boolean[] = [false, false];
	/** What each element is for while it holds a clip that can play: its part of the film and its role. */
	let roles: ({ part: FilmPart; role: Role } | null)[] = [null, null];
	/** How far each element's picture is ahead of the sound, in seconds, smoothed over its last frames. */
	let ahead = [0, 0];
	/** Whether an element's picture was found far off the sound: it is sought at the next look. */
	let lost = [false, false];
	/** The jump of the transport each element last followed, and when it was last sought or started. */
	let leapt = [-1, -1];
	let touched = [0, 0];
	/** Whether an element runs at a crawl, waiting for its cut. */
	let crawling = [false, false];
	/** A clip that becomes the next one right at a cut waits a moment, so nothing loads while the cut is shown. */
	let notBefore = [0, 0];
	let looks: [string, string][] = [
		['', ''],
		['', '']
	];
	let clockText = '';
	let slate = $state<{ n: number; state: keyof typeof STATE_WORD; text: string } | null>(null);
	let shown = $state(-1);
	let caption = $state('');
	/** The picture on the screen, measured: the subtitle's letters are a share of its height, as in an export. */
	let screen = $state<HTMLDivElement | null>(null);
	let letters = $state(12);
	$effect(() => {
		if (!screen) return;
		const el = screen;
		const measure = () => (letters = Math.max(8, Math.round(el.clientHeight * 0.052 * 10) / 10));
		const seen = new ResizeObserver(measure);
		seen.observe(el);
		measure();
		return () => seen.disconnect();
	});
	let clock = $state<HTMLSpanElement | null>(null);

	const [w, h] = $derived((studio.doc?.size ?? '640x384').split('x').map(Number));

	const clipKey = (p: FilmPart) => {
		const s = studio.shots[p.index];
		return `${p.shot}:${s?.take ?? 0}:${s?.seconds ?? 0}`;
	};

	/** How an element looks: written only when it changes, so a playing film lays nothing out again. */
	function look(slot: number, el: HTMLVideoElement, seen: number, role: Role) {
		const opacity = String(Math.round(seen * 1000) / 1000);
		const z = role === 'now' ? '2' : '1';
		if (looks[slot][0] !== opacity) el.style.opacity = opacity;
		if (looks[slot][1] !== z) el.style.zIndex = z;
		looks[slot] = [opacity, z];
	}

	function start(slot: number, el: HTMLVideoElement, rate = 1) {
		speed(el, rate);
		crawling[slot] = rate !== 1;
		ahead[slot] = 0;
		touched[slot] = performance.now();
		el.play().catch(() => {});
	}

	/** A webview that cannot play that slowly plays as slowly as it can. */
	function speed(el: HTMLVideoElement, rate: number) {
		try {
			el.playbackRate = rate;
		} catch {
			el.playbackRate = rate < 1 ? 0.25 : rate;
		}
	}

	/** Puts a part into an element. `role` says whether it is on screen now, fading out, or waiting. */
	function hold(slot: number, p: FilmPart | undefined, role: Role, time: number, seen: number, heard: number) {
		const el = videos[slot];
		if (!el) return;
		if (!p || !p.made) {
			roles[slot] = null;
			look(slot, el, 0, role);
			if (!el.paused) el.pause();
			return;
		}
		// The shot's own sound is read in time, and heard at the level of the moment.
		studio.shotSound(p.shot);
		studio.audio.level(p.shot, heard);
		const key = clipKey(p);
		if (held[slot] !== key) {
			// The element that was on screen until a cut takes the clip after next only a moment later.
			if (studio.playing && role === 'next' && roles[slot] && roles[slot]!.role !== 'next') notBefore[slot] = performance.now() + 350;
			roles[slot] = null;
			look(slot, el, 0, role);
			if (!el.paused) el.pause();
			if (studio.playing && role === 'next' && performance.now() < notBefore[slot]) return;
			held[slot] = key;
			ready[slot] = false;
			studio.clipOf(p.shot).then((url) => {
				if (held[slot] !== key || !url) return;
				el.src = url;
				el.onloadeddata = () => {
					ready[slot] = true;
					sync(studio.time);
				};
			});
			return;
		}
		if (!ready[slot]) return;
		roles[slot] = { part: p, role };
		const at = role === 'next' ? p.from : Math.min(p.to - 0.001, p.from + Math.max(0, time - p.start));
		// The next shot runs, hidden and silent, at a crawl before its cut; at the cut its speed is the film's.
		const early = role === 'next' && !studio.starting && p.start - time <= PREROLL && (el.paused || leapt[slot] === studio.leaps);
		const moving = studio.playing && (role !== 'next' || early) && time < p.start + p.length;
		if (moving) {
			const jumped = role !== 'next' && leapt[slot] !== studio.leaps;
			leapt[slot] = studio.leaps;
			if (role !== 'next' && crawling[slot]) {
				crawling[slot] = false;
				speed(el, 1);
			}
			if (el.paused) {
				if (Math.abs(el.currentTime - at) > 0.04 && !el.seeking) el.currentTime = at;
				start(slot, el, role === 'next' ? CRAWL : 1);
			} else if (jumped && Math.abs(el.currentTime - at) > 0.04) {
				el.currentTime = at;
				ahead[slot] = 0;
				touched[slot] = performance.now();
			} else if (!FRAMES) follow(slot, el, el.currentTime, performance.now());
			else if (lost[slot] && performance.now() - touched[slot] > 1000) {
				// Far off: sought a little ahead, as seeking takes a moment of its own.
				el.currentTime = Math.min(p.to - 0.001, at + 0.1);
				ahead[slot] = 0;
				touched[slot] = performance.now();
			}
			lost[slot] = false;
		} else {
			if (!el.paused) el.pause();
			crawling[slot] = false;
			if (el.playbackRate !== 1) el.playbackRate = 1;
			if (Math.abs(el.currentTime - at) > 0.015 && !el.seeking) el.currentTime = at;
		}
		look(slot, el, seen, role);
	}

	/**
	 * A frame of an element was put on the screen (`shownAt`, a moment of performance.now()): how
	 * far it is from the sound, and the element's speed nudged to close the gap.
	 */
	function follow(slot: number, el: HTMLVideoElement, media: number, shownAt: number) {
		const r = roles[slot];
		if (!r || el.paused || el.seeking || !studio.playing || crawling[slot]) return;
		const film = r.part.start + (media - r.part.from);
		if (studio.starting) {
			// The sound starts with the first picture of the shot on screen.
			if (r.role === 'now' && Math.abs(film - studio.time) < FAR) studio.soundFrom(film, shownAt);
			return;
		}
		const off = film - studio.audio.heardAt(shownAt);
		if (Math.abs(off) > FAR) {
			lost[slot] = true;
			return;
		}
		ahead[slot] = ahead[slot] * 0.7 + off * 0.3;
		const gap = Math.abs(ahead[slot]);
		const rate = el.playbackRate;
		// Within a third of a frame nothing is done; past it the speed changes, by at most a quarter. The
		// element is silent (the mixer plays the shot's sound), so its speed is heard nowhere.
		let want = rate;
		if (gap < 0.008) want = 1;
		else if (gap > 0.014 || rate !== 1) want = Math.round(Math.max(0.8, Math.min(1.25, 1 - ahead[slot] * 2.5)) * 100) / 100;
		if (want !== rate) el.playbackRate = want;
	}

	/** Every frame an element shows is measured against the sound. */
	function watch(el: HTMLVideoElement, slot: number) {
		if (!FRAMES) return;
		let gone = false;
		const each = (_now: number, meta: VideoFrameCallbackMetadata) => {
			if (gone) return;
			follow(slot, el, meta.mediaTime, meta.expectedDisplayTime);
			el.requestVideoFrameCallback(each);
		};
		el.requestVideoFrameCallback(each);
		return () => (gone = true);
	}
	$effect(() => {
		const stops = videos.map((el, slot) => (el ? watch(el, slot) : undefined));
		return () => stops.forEach((stop) => stop?.());
	});

	/** Shows the film at a moment: which parts are on screen, how much of each is seen and heard. */
	function sync(time: number) {
		const doc = studio.doc;
		const { parts, total } = studio.plan;
		const text = timecode(time);
		if (clock && text !== clockText) clock.textContent = clockText = text;
		const words = cueAt(studio.cues, time)?.text ?? '';
		if (words !== caption) caption = words;
		if (!doc || parts.length === 0) {
			for (const slot of [0, 1]) hold(slot, undefined, 'next', 0, 0, 0);
			slate = null;
			shown = -1;
			if (studio.starting) studio.soundFrom(time);
			return;
		}
		let now = 0;
		for (let i = 0; i < parts.length; i++) if (parts[i].start <= time + 1e-6) now = i;
		const p = parts[now];
		// Nothing to show at the playhead: the sound starts by itself.
		if (studio.starting && (!p.made || time >= total)) studio.soundFrom(time);
		const before = now > 0 && p.overlap > 0 && time < p.start + p.overlap ? parts[now - 1] : undefined;
		const mix = before ? Math.min(1, Math.max(0, (time - p.start) / p.overlap)) : 1;
		const edges: [number, number] = [doc.fade_in ? EDGE : 0, doc.fade_out ? EDGE : 0];
		const light = partLight(p, Math.min(time, total), total, ...edges);
		// The shots' own sound goes down while a line is said.
		const duck = duckLevel(studio.said, doc.duck, time);
		hold(now % 2, p, 'now', time, light * (before?.made ? mix : 1), p.volume * light * mix * duck);
		if (before) {
			const under = partLight(before, time, total, ...edges);
			hold((now + 1) % 2, before, 'out', time, under, before.volume * under * (1 - mix) * duck);
		} else {
			// The next shot is heard from its first moment at the level it starts with.
			const next = parts[now + 1];
			const first = next ? next.volume * partLight(next, next.start, total, ...edges) * duckLevel(studio.said, doc.duck, next.start) : 0;
			hold((now + 1) % 2, next, 'next', time, 0, first);
		}
		if (shown !== now || (slate !== null) !== !p.made) {
			shown = now;
			const s = studio.shots[p.index];
			slate = p.made ? null : { n: p.index + 1, state: (s?.state ?? 'empty') as keyof typeof STATE_WORD, text: s?.prompt ?? '' };
		}
	}

	const stopFrames = studio.onFrame(sync);

	// The film changed under the playhead, or it started or stopped: show it as it is now.
	$effect(() => {
		void studio.plan;
		void studio.jumps;
		void studio.playing;
		void studio.doc?.fade_in;
		void studio.doc?.fade_out;
		void studio.doc?.duck;
		void studio.cues;
		void studio.said;
		void videos[0];
		void videos[1];
		for (const p of studio.plan.parts) void studio.shots[p.index]?.state;
		sync(studio.time);
	});

	onDestroy(() => {
		stopFrames();
		for (const el of videos) el?.pause();
	});
</script>

<div class="viewer" class:large={studio.viewerLarge} role="group" aria-label="Viewer: the film at the playhead">
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="screen" bind:this={screen} style:aspect-ratio="{w} / {h}" onclick={() => studio.togglePlay()} ondblclick={() => (studio.viewerLarge = !studio.viewerLarge)}>
		{#each [0, 1] as slot (slot)}
			<!-- svelte-ignore a11y_media_has_caption -->
			<video bind:this={videos[slot]} preload="auto" playsinline muted onseeked={() => !studio.playing && sync(studio.time)}></video>
		{/each}
		{#if slate}
			<div class="slate">
				<span class="sn">Shot {slate.n}</span>
				<span class="ss {slate.state}">{STATE_WORD[slate.state]}</span>
				{#if slate.text}<p>{slate.text}</p>{/if}
			</div>
		{:else if studio.plan.parts.length === 0}
			<div class="slate"><span class="ss">No shots yet</span></div>
		{/if}
		{#if caption}<p class="caption" style:font-size="{letters}px" aria-live="off">{caption}</p>{/if}
	</div>
	<div class="bar">
		<button class="vb" onclick={() => studio.togglePlay()} aria-label={studio.playing ? 'Pause' : 'Play'} use:tag={{ id: 'play', name: studio.playing ? 'Pause' : 'Play' }}><StudioIcon name={studio.playing ? 'pause' : 'play'} size={11} /></button>
		<span class="clock" bind:this={clock}>00:00:00:00</span>
		<span class="what">{shown >= 0 ? `Shot ${studio.plan.parts[shown]?.index + 1} of ${studio.doc?.shots.length ?? 0}` : ''}</span>
		{#if studio.viewerLarge && !studio.exportCurrent && studio.exportPlan.parts.length > 0}
			<button class="st-btn primary vx" onclick={() => studio.exportFilm()} disabled={studio.view?.export?.state === 'working'} use:tag={'export'}>
				{studio.view?.export?.state === 'working' ? 'Exporting…' : 'Export'}
			</button>
		{/if}
		<button class="vb" onclick={() => (studio.viewerLarge = !studio.viewerLarge)} aria-label={studio.viewerLarge ? 'Dock the viewer' : 'Make the viewer large'} use:tag={{ id: 'viewer', name: studio.viewerLarge ? 'Dock the viewer' : 'Large viewer', what: studio.viewerLarge ? 'Puts the picture back above the Inspector.' : 'Shows the picture large over the window.' }}>
			<StudioIcon name={studio.viewerLarge ? 'shrink' : 'expand'} size={12} />
		</button>
	</div>
</div>

<style>
	/* The subtitle of the moment, set as an export that holds its subtitles in the picture sets it. */
	.caption {
		position: absolute;
		z-index: 4;
		left: 7%;
		right: 7%;
		bottom: 5%;
		margin: 0;
		text-align: center;
		white-space: pre-line;
		font: 600 12px/1.3 var(--rb-text);
		color: #fff;
		text-shadow:
			0 0 0.12em #000,
			0 0 0.12em #000,
			0 0.06em 0.2em rgba(0, 0, 0, 0.9);
		pointer-events: none;
	}

	.viewer {
		flex: none;
		display: flex;
		flex-direction: column;
		min-height: 0;
		height: 100%;
		background: #000;
	}

	/* Large: over the middle of the workstation, on the same elements, so the film goes on playing. */
	.viewer.large {
		position: fixed;
		z-index: 30;
		visibility: visible;
		left: calc(var(--dx) + var(--dw) * 0.12);
		top: calc(var(--dy) + var(--dh) * 0.09);
		width: calc(var(--dw) * 0.76);
		height: calc(var(--dh) * 0.8);
		border: 1px solid #000;
		border-radius: 4px;
		box-shadow:
			0 0 0 1px rgba(255, 255, 255, 0.08),
			0 24px 80px rgba(0, 0, 0, 0.75),
			0 0 0 100vmax rgba(0, 0, 0, 0.5);
		overflow: hidden;
	}

	.screen {
		position: relative;
		flex: 1 1 auto;
		min-height: 0;
		max-width: 100%;
		max-height: calc(100% - 22px);
		margin: 0 auto;
		background: #000;
		overflow: hidden;
	}

	video {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		opacity: 0;
		background: transparent;
	}

	.slate {
		position: absolute;
		inset: 0;
		z-index: 3;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 4px;
		padding: 10px 14px;
		text-align: center;
		background: repeating-linear-gradient(135deg, #111214 0 9px, #141518 9px 18px);
	}

	.sn {
		font: 600 12px/1 var(--rb-data);
		color: var(--st-ink);
	}

	.ss {
		font-size: 10.5px;
		color: var(--st-muted);
	}

	.ss.failed {
		color: var(--st-fail);
	}

	.ss.making,
	.ss.queued {
		color: var(--st-accent);
	}

	.slate p {
		max-width: 46ch;
		font-size: 10.5px;
		line-height: 1.35;
		color: var(--st-faint);
		display: -webkit-box;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		-webkit-box-orient: vertical;
		overflow: hidden;
	}

	.large .sn {
		font-size: 18px;
	}

	.large .ss,
	.large .slate p {
		font-size: 13px;
	}

	.bar {
		flex: none;
		height: 22px;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 4px;
		background: var(--st-panel);
		border-top: 1px solid var(--st-edge);
	}

	.vb {
		width: 20px;
		height: 18px;
		display: grid;
		place-items: center;
		border-radius: 2.5px;
		color: var(--st-muted);
	}

	.vb:hover {
		color: var(--st-ink);
		background: rgba(255, 255, 255, 0.09);
	}

	.vx {
		height: 18px;
		padding: 0 8px;
		font-size: 10.5px;
	}

	.clock {
		font: 500 10.5px/1 var(--rb-data);
		font-variant-numeric: tabular-nums;
		color: var(--st-ink);
	}

	.what {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 10.5px;
		color: var(--st-faint);
	}
</style>
