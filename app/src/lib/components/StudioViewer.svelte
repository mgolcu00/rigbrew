<script lang="ts">
	import { onDestroy } from 'svelte';
	import { cueAt, duckLevel } from '#lib/studio-edit.ts';
	import { STATE_WORD } from '#lib/studio-look.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { timecode } from '#lib/studio-time.ts';
	import { partLight, type FilmPart } from '#lib/studio.ts';
	import StudioIcon from './StudioIcon.svelte';

	// The picture of the film at the playhead, as the export will hold it: every shot from its trim
	// start to its trim end, dissolves as two pictures over each other, fades through black, the
	// film's own fades. Two video elements take turns: while one shows a shot, the other holds the
	// next one ready. They follow the workstation's clock; their sound goes into the mixer, turned
	// down under what is said as the export turns it down. The subtitle of the moment lies over it.

	const EDGE = 0.5; // seconds of the film's own fade in and fade out

	let videos = $state<(HTMLVideoElement | null)[]>([null, null]);
	/** The clip each element holds, and the part of the film it is for. */
	let held: string[] = ['', ''];
	let ready: boolean[] = [false, false];
	let gains: (GainNode | null)[] = [null, null];
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

	/** Puts a part into an element. `role` says whether it is on screen now, fading out, or waiting. */
	function hold(slot: number, p: FilmPart | undefined, role: 'now' | 'out' | 'next', time: number, seen: number, heard: number) {
		const el = videos[slot];
		if (!el) return;
		if (!p || !p.made) {
			el.style.opacity = '0';
			if (!el.paused) el.pause();
			return;
		}
		const key = clipKey(p);
		if (held[slot] !== key) {
			held[slot] = key;
			ready[slot] = false;
			el.pause();
			el.style.opacity = '0';
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
		const at = role === 'next' ? p.from : Math.min(p.to - 0.001, p.from + Math.max(0, time - p.start));
		const playing = studio.playing && role !== 'next' && time < p.start + p.length;
		if (playing) {
			if (el.paused) {
				if (Math.abs(el.currentTime - at) > 0.04) el.currentTime = at;
				el.play().catch(() => {});
			} else if (Math.abs(el.currentTime - at) > 0.14) el.currentTime = at;
		} else {
			if (!el.paused) el.pause();
			if (Math.abs(el.currentTime - at) > 0.015 && !el.seeking) el.currentTime = at;
		}
		el.style.opacity = String(seen);
		el.style.zIndex = role === 'now' ? '2' : '1';
		// The sound goes through the mixer once it runs; before that nothing plays anyway.
		if (studio.playing && !gains[slot] && studio.audio.ctx) gains[slot] = studio.audio.element(el);
		const gain = gains[slot];
		if (gain) gain.gain.value = role === 'next' ? 0 : heard;
	}

	/** Shows the film at a moment: which parts are on screen, how much of each is seen and heard. */
	function sync(time: number) {
		const doc = studio.doc;
		const { parts, total } = studio.plan;
		if (clock) clock.textContent = timecode(time);
		const words = cueAt(studio.cues, time)?.text ?? '';
		if (words !== caption) caption = words;
		if (!doc || parts.length === 0) {
			for (const slot of [0, 1]) hold(slot, undefined, 'next', 0, 0, 0);
			slate = null;
			shown = -1;
			return;
		}
		let now = 0;
		for (let i = 0; i < parts.length; i++) if (parts[i].start <= time + 1e-6) now = i;
		const p = parts[now];
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
		} else hold((now + 1) % 2, parts[now + 1], 'next', time, 0, 0);
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
			<video bind:this={videos[slot]} preload="auto" playsinline onseeked={() => !studio.playing && sync(studio.time)}></video>
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
		<button class="vb" onclick={() => studio.togglePlay()} aria-label={studio.playing ? 'Pause' : 'Play'} title="{studio.playing ? 'Pause' : 'Play'} (Space)"><StudioIcon name={studio.playing ? 'pause' : 'play'} size={11} /></button>
		<span class="clock" bind:this={clock}>00:00:00:00</span>
		<span class="what">{shown >= 0 ? `Shot ${studio.plan.parts[shown]?.index + 1} of ${studio.doc?.shots.length ?? 0}` : ''}</span>
		<button class="vb" onclick={() => (studio.viewerLarge = !studio.viewerLarge)} aria-label={studio.viewerLarge ? 'Dock the viewer' : 'Make the viewer large'} title="{studio.viewerLarge ? 'Dock the viewer' : 'Large viewer'} (V)">
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
