<script lang="ts">
	import { onDestroy, onMount, untrack } from 'svelte';
	import { partAt, stamp, type FilmPart } from '#lib/studio.ts';

	// Plays the film as the export will hold it: the made shots back to back, each from its trim
	// start to its trim end, with the sound file under them. Two video elements take turns: while
	// one plays, the other holds the next shot ready at its first picture, so there is no gap.

	let {
		parts,
		total,
		shape,
		clipOf,
		sound = null,
		soundVolume = 0.5,
		soundRepeats = true,
		fadeIn = false,
		fadeOut = false,
		caption,
		onposition,
		onclose
	}: {
		parts: FilmPart[];
		total: number;
		/** "640x384" */
		shape: string;
		/** The clip of a shot as something a video element can play. */
		clipOf: (shot: string) => Promise<string | null>;
		/** The film's sound file, as something an audio element can play. */
		sound?: string | null;
		soundVolume?: number;
		soundRepeats?: boolean;
		/** The film comes up from black at its start and goes down to black at its end. */
		fadeIn?: boolean;
		fadeOut?: boolean;
		caption: (part: FilmPart) => string;
		onposition: (at: { shot: string; at: number } | null) => void;
		onclose: () => void;
	} = $props();

	const EDGE = 0.5; // seconds of the film's own fade in and fade out

	let videos = $state<(HTMLVideoElement | null)[]>([null, null]);
	let music = $state<HTMLAudioElement | null>(null);
	/** Which of the two video elements shows the current part, and which part each one holds. */
	let front = $state(0);
	let held: number[] = [-1, -1];
	let current = $state(0);
	let incoming = false;
	let playing = $state(false);
	let time = $state(0);
	let frame = 0;
	let alive = true;

	const [width, height] = $derived(shape.split('x').map(Number));

	/** Puts a part into one of the two video elements, standing at the right picture. */
	async function hold(slot: number, part: number, at?: number) {
		const el = videos[slot];
		const p = parts[part];
		if (!el || !p) {
			held[slot] = -1;
			return;
		}
		const where = at ?? p.from;
		if (held[slot] === part && el.src) {
			if (Math.abs(el.currentTime - where) > 0.02) el.currentTime = where;
			return;
		}
		held[slot] = part;
		const url = await clipOf(p.shot);
		if (!alive || held[slot] !== part || !url) return;
		el.src = url;
		el.currentTime = where;
		// It was wanted on screen before it was ready.
		if (playing && (slot === front || incoming)) el.play().catch(() => {});
	}

	/** How bright a part is at a moment of its clip: its fades through black, and the film's own. */
	function light(p: FilmPart, at: number, film: number) {
		let level = 1;
		if (p.fadeIn > 0) level = Math.min(level, (at - p.from) / p.fadeIn);
		if (p.fadeOut > 0) level = Math.min(level, (p.to - at) / p.fadeOut);
		if (fadeIn) level = Math.min(level, film / EDGE);
		if (fadeOut) level = Math.min(level, (total - film) / EDGE);
		return Math.max(0, Math.min(1, level));
	}

	/** How much of a video element is seen and heard. */
	function dress(slot: number, seen: number, heard: number) {
		const el = videos[slot];
		const p = parts[held[slot]];
		if (!el || !p) return;
		el.style.opacity = String(seen);
		// A video element is at most as loud as the clip; the export goes up to 150%.
		el.volume = Math.max(0, Math.min(1, p.volume * heard));
		el.muted = p.volume === 0;
	}

	/** Once per picture while the film plays. */
	function tick() {
		frame = 0;
		step();
		if (playing && alive) frame = requestAnimationFrame(tick);
	}

	/** Looks where the film is: starts the next part, mixes a join, switches at the end of a part. */
	function step() {
		if (!alive) return;
		const el = videos[front];
		const p = parts[current];
		if (el && p) {
			const at = Math.min(p.to, Math.max(p.from, el.currentTime));
			time = p.start + at - p.from;
			const back = 1 - front;
			const next = parts[current + 1];
			// The next part starts while this one is still on screen when the two dissolve.
			if (playing && next && !incoming && time >= next.start && held[back] === current + 1) {
				incoming = true;
				videos[back]?.play().catch(() => {});
			}
			const mix = incoming && next && next.overlap > 0 ? Math.min(1, (time - next.start) / next.overlap) : 0;
			// In a dissolve the next part lies under this one, which thins out over it.
			const level = light(p, at, time);
			dress(front, level * (1 - mix), level * (1 - mix));
			if (incoming && next && next.overlap > 0) {
				const under = light(next, videos[back]?.currentTime ?? next.from, time);
				dress(back, under, under * mix);
			} else if (videos[back]) videos[back]!.style.opacity = '0';
			if (playing && (el.currentTime >= p.to - 0.01 || el.ended)) {
				el.pause();
				if (next) {
					current += 1;
					front = back;
					if (!incoming) videos[front]?.play().catch(() => {});
					incoming = false;
					// On screen at once; the next look sets its fades.
					if (videos[front]) videos[front]!.style.opacity = String(light(next, next.from, time));
					el.style.opacity = '0';
					hold(1 - front, current + 1);
				} else {
					playing = false;
					time = total;
					el.style.opacity = String(light(p, p.to, total));
					music?.pause();
				}
			}
			onposition({ shot: p.shot, at });
			follow();
		}
	}

	/** Keeps the sound file where the film is. */
	function follow(force = false) {
		if (!music || !sound) return;
		music.volume = Math.max(0, Math.min(1, soundVolume * (fadeIn ? Math.min(1, time / EDGE) : 1) * (fadeOut ? Math.min(1, (total - time) / EDGE) : 1)));
		const length = music.duration;
		if (!Number.isFinite(length) || length <= 0) return;
		const over = !soundRepeats && time >= length;
		const want = soundRepeats ? time % length : Math.min(time, length);
		if (force || Math.abs(music.currentTime - want) > 0.3) music.currentTime = want;
		if (playing && !over && music.paused) music.play().catch(() => {});
		if ((!playing || over) && !music.paused) music.pause();
	}

	export async function seek(seconds: number) {
		const where = partAt(parts, Math.max(0, Math.min(total, seconds)));
		if (!where) return;
		for (const v of videos) v?.pause();
		incoming = false;
		current = where.part;
		time = parts[current].start + where.at - parts[current].from;
		await hold(front, current, where.at);
		hold(1 - front, current + 1);
		follow(true);
		if (playing) videos[front]?.play().catch(() => (playing = false));
		if (!frame) frame = requestAnimationFrame(tick);
	}

	/** Goes to the first picture of a shot, when the shot is in the film. */
	export function seekShot(shot: string) {
		const p = parts.find((x) => x.shot === shot);
		if (p) seek(p.start + p.overlap);
	}

	async function toggle() {
		if (playing) {
			playing = false;
			for (const v of videos) v?.pause();
			music?.pause();
			return;
		}
		if (time >= total - 0.05) await seek(0);
		playing = true;
		try {
			await videos[front]?.play();
		} catch {
			// The system did not let it start by itself: the Play button is still there.
			playing = false;
		}
		follow(true);
		if (!frame) frame = requestAnimationFrame(tick);
	}

	function keys(e: KeyboardEvent) {
		if (e.key === ' ' && !(e.target instanceof HTMLButtonElement)) {
			e.preventDefault();
			toggle();
		} else if (e.key === 'Escape') onclose();
	}

	// Pictures are not drawn while the window is hidden, and the film must still go from shot to
	// shot: a slow beat and the end of each clip keep it going.
	let beat: ReturnType<typeof setInterval> | undefined;

	onMount(async () => {
		beat = setInterval(() => playing && step(), 200);
		await seek(0);
		toggle();
	});

	// A trim, a join or a new clip changes the film under the player: stand at the same moment of the new one.
	const filmKey = $derived(JSON.stringify(parts));
	let lastKey = '';
	$effect(() => {
		const now = filmKey;
		untrack(() => {
			if (lastKey && lastKey !== now) {
				held = [-1, -1];
				seek(time);
			}
			lastKey = now;
		});
	});

	onDestroy(() => {
		alive = false;
		clearInterval(beat);
		cancelAnimationFrame(frame);
		onposition(null);
	});
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<div class="player" role="group" aria-label="The film" onkeydown={keys}>
	<div class="screen" style:aspect-ratio="{width} / {height}" style:max-width="{Math.round((420 * width) / height)}px">
		{#each [0, 1] as slot (slot)}
			<!-- svelte-ignore a11y_media_has_caption -->
			<video bind:this={videos[slot]} class:front={slot === front} preload="auto" playsinline onended={() => playing && step()}></video>
		{/each}
	</div>
	{#if sound}<audio bind:this={music} src={sound} preload="auto" onloadedmetadata={() => follow(true)}></audio>{/if}
	<div class="bar">
		<button class="btn" onclick={toggle}>{playing ? 'Pause' : 'Play'}</button>
		<input type="range" min="0" max={total} step="0.01" value={time} oninput={(e) => seek(Number(e.currentTarget.value))} aria-label="Place in the film" aria-valuetext="{stamp(time)} of {stamp(total)}" />
		<span class="clock">{stamp(time)} / {stamp(total)}</span>
		<button class="btn quiet" onclick={onclose}>Close</button>
	</div>
	{#if parts[current]}<p class="caption">{caption(parts[current])}</p>{/if}
</div>

<style>
	.player {
		display: grid;
		gap: 8px;
		padding: 12px;
		border-radius: 14px;
		background: var(--well);
	}

	.screen {
		position: relative;
		width: 100%;
		margin: 0 auto;
		overflow: hidden;
		border-radius: 8px;
		background: #000;
	}

	video {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		opacity: 0;
	}

	video.front {
		z-index: 1;
	}

	.bar {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	input[type='range'] {
		flex: 1;
		min-width: 0;
		accent-color: var(--accent);
	}

	.clock {
		font: 400 12px/1 var(--rb-data);
		color: var(--muted);
		white-space: nowrap;
	}

	.caption {
		font-size: 13px;
		color: var(--muted);
		overflow: hidden;
		display: -webkit-box;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		-webkit-box-orient: vertical;
	}
</style>
