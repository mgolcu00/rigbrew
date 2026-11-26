<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import * as api from '#lib/api.ts';
	import { bytes as bytesOf } from '#lib/format.ts';
	import { remembered } from '#lib/remember.ts';
	import { fileName } from '#lib/studio-look.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { isMac } from '#lib/words.ts';
	import ErrorLine from './ErrorLine.svelte';
	import StudioIcon from './StudioIcon.svelte';

	// The exported film, whole, in a sheet over the workstation. The webview's own video element
	// plays it from an address the service answers in parts, so a long film starts at once and is
	// never read into memory as one piece.

	let sheet = $state<HTMLDivElement | null>(null);
	let video = $state<HTMLVideoElement | null>(null);
	let src = $state('');
	let problem = $state<unknown>(null);
	let playing = $state(false);
	let time = $state(0);
	let duration = $state(0);
	let volume = $state(Math.min(1, Math.max(0, remembered.json('studio.playerVolume', 1))));
	let muted = $state(false);
	let full = $state(false);
	/** Where to go on when the address is asked for again (it lasts while it is used, not for ever). */
	let resume = 0;
	let again = false;
	let frame = 0;
	const film = $derived(studio.view?.film ?? null);

	const clock = (s: number) => {
		const t = Math.max(0, Math.floor(Number.isFinite(s) ? s : 0));
		const two = (n: number) => String(n).padStart(2, '0');
		return t >= 3600 ? `${Math.floor(t / 3600)}:${two(Math.floor(t / 60) % 60)}:${two(t % 60)}` : `${Math.floor(t / 60)}:${two(t % 60)}`;
	};

	async function load() {
		problem = null;
		try {
			src = (await api.studioMedia(studio.id)).url;
		} catch (e) {
			problem = e;
		}
	}

	function loaded() {
		if (!video) return;
		duration = video.duration;
		video.volume = volume;
		if (resume > 0) video.currentTime = resume;
		// Opened by the person: it plays. Opened by itself after an export: it waits for them.
		if (studio.player === 'asked' || resume > 0) video.play().catch(() => {});
		resume = 0;
	}

	function failed() {
		if (!video) return;
		// The address may have gone after a long pause: ask for a new one once and go on from here.
		if (!again) {
			again = true;
			resume = video.currentTime || time;
			load();
			return;
		}
		problem = new Error(video.error?.message || 'The film could not be played.');
	}

	function toggle() {
		if (!video) return;
		if (video.paused) video.play().catch((e) => (problem = e));
		else video.pause();
	}

	function seek(to: number) {
		if (!video || !Number.isFinite(duration)) return;
		video.currentTime = Math.min(Math.max(0, to), duration);
		time = video.currentTime;
	}

	function setVolume(v: number) {
		volume = v;
		if (video) {
			video.volume = v;
			video.muted = v === 0;
		}
		remembered.setJson('studio.playerVolume', v);
	}

	async function fullscreen() {
		if (!sheet) return;
		try {
			if (document.fullscreenElement) await document.exitFullscreen();
			else await sheet.requestFullscreen();
		} catch {
			// Not allowed here: the sheet already fills the window.
		}
	}

	function close() {
		video?.pause();
		if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
		studio.player = '';
	}

	function keys(e: KeyboardEvent) {
		if (e.defaultPrevented || e.metaKey || e.ctrlKey || e.altKey) return;
		const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
		if (key === ' ') toggle();
		else if (key === 'ArrowLeft') seek(time - (e.shiftKey ? 1 : 5));
		else if (key === 'ArrowRight') seek(time + (e.shiftKey ? 1 : 5));
		else if (key === 'f') fullscreen();
		else if (key === 'Escape') {
			if (document.fullscreenElement) document.exitFullscreen().catch(() => {});
			else close();
		} else return;
		e.preventDefault();
		if (e.target instanceof HTMLButtonElement) e.target.blur();
	}

	// The clock and the bar move every frame while the film plays.
	function follow() {
		if (video) time = video.currentTime;
		if (playing) frame = requestAnimationFrame(follow);
	}

	const onFull = () => (full = document.fullscreenElement === sheet);

	onMount(() => {
		document.addEventListener('fullscreenchange', onFull);
		load();
	});

	onDestroy(() => {
		cancelAnimationFrame(frame);
		document.removeEventListener('fullscreenchange', onFull);
		if (video) {
			video.pause();
			// Let go of the address, so nothing more is read from it.
			video.removeAttribute('src');
			video.load();
		}
	});
</script>

<svelte:window onkeydown={keys} />

<div class="sheet" class:full bind:this={sheet} role="dialog" aria-label="The exported film">
	<header>
		<StudioIcon name="film" size={14} />
		<span class="name">{studio.doc?.name ?? ''}</span>
		{#if film}<span class="file selectable" title={film.file}>{fileName(film.file)}, {bytesOf(film.bytes)}</span>{/if}
		<span class="grow"></span>
		<button class="st-btn quiet" onclick={() => studio.reveal()}><StudioIcon name="folder" size={12} /> {isMac ? 'Show in Finder' : 'Show in folder'}</button>
		<button class="x" onclick={close} aria-label="Close the player" title="Close (Esc)"><StudioIcon name="close" size={12} /></button>
	</header>
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="screen" onclick={toggle} ondblclick={fullscreen}>
		{#if src}
			<!-- svelte-ignore a11y_media_has_caption -->
			<video
				bind:this={video}
				{src}
				preload="auto"
				playsinline
				onloadedmetadata={loaded}
				ondurationchange={() => video && (duration = video.duration)}
				onplay={() => ((playing = true), follow())}
				onpause={() => (playing = false)}
				onended={() => (playing = false)}
				ontimeupdate={() => !playing && video && (time = video.currentTime)}
				onvolumechange={() => video && (muted = video.muted)}
				onerror={failed}
			></video>
		{/if}
	</div>
	{#if problem}
		<div class="problem"><ErrorLine error={problem} lead="The film could not be played." where="studio" compact onretry={() => ((again = false), load())} /></div>
	{/if}
	<div class="bar">
		<button class="pb" onclick={toggle} aria-label={playing ? 'Pause' : 'Play'} title="{playing ? 'Pause' : 'Play'} (Space)" disabled={!src}>
			<StudioIcon name={playing ? 'pause' : 'play'} size={14} />
		</button>
		<span class="time">{clock(time)} / {clock(duration)}</span>
		<input
			class="scrub"
			type="range"
			min="0"
			max={Number.isFinite(duration) && duration > 0 ? duration : 0}
			step="0.01"
			value={time}
			oninput={(e) => seek(Number(e.currentTarget.value))}
			aria-label="Where in the film"
			title="Left and Right: five seconds back or on"
			disabled={!src}
		/>
		<button class="pb" class:off={muted || volume === 0} onclick={() => video && (video.muted = !video.muted)} aria-label={muted ? 'Sound on' : 'Sound off'} title={muted ? 'Sound on' : 'Sound off'}>
			<StudioIcon name="speaker" size={14} />
		</button>
		<input class="volume" type="range" min="0" max="1" step="0.01" value={muted ? 0 : volume} oninput={(e) => setVolume(Number(e.currentTarget.value))} aria-label="Volume" />
		<button class="pb" onclick={fullscreen} aria-label={full ? 'Leave full screen' : 'Full screen'} title="{full ? 'Leave full screen' : 'Full screen'} (F)">
			<StudioIcon name={full ? 'shrink' : 'expand'} size={14} />
		</button>
	</div>
</div>

<style>
	.sheet {
		position: absolute;
		inset: 0;
		z-index: 40;
		display: flex;
		flex-direction: column;
		background: #000;
		color: var(--st-ink);
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 8px;
		height: 38px;
		padding: 0 8px 0 12px;
		background: var(--st-panel);
		border-bottom: 1px solid var(--st-edge);
		color: var(--st-muted);
	}

	.name {
		font: 600 12px/1 var(--rb-text);
		color: var(--st-ink);
		white-space: nowrap;
	}

	.file {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 11px;
		color: var(--st-faint);
	}

	.grow {
		flex: 1;
	}

	.x {
		width: 26px;
		height: 24px;
		display: grid;
		place-items: center;
		border-radius: 3.5px;
		color: var(--st-muted);
	}

	.x:hover {
		color: var(--st-ink);
		background: rgba(255, 255, 255, 0.09);
	}

	.screen {
		position: relative;
		flex: 1 1 auto;
		min-height: 0;
		cursor: pointer;
	}

	video {
		position: absolute;
		inset: 0;
		width: 100%;
		height: 100%;
		object-fit: contain;
		background: #000;
	}

	.problem {
		flex: none;
		padding: 6px 12px;
		background: var(--st-panel);
	}

	.bar {
		flex: none;
		display: flex;
		align-items: center;
		gap: 10px;
		height: 42px;
		padding: 0 12px;
		background: var(--st-panel);
		border-top: 1px solid var(--st-edge);
	}

	.pb {
		flex: none;
		width: 30px;
		height: 26px;
		display: grid;
		place-items: center;
		border-radius: 3.5px;
		color: var(--st-ink);
	}

	.pb:hover:not(:disabled) {
		background: rgba(255, 255, 255, 0.1);
	}

	.pb.off {
		color: var(--st-faint);
	}

	.time {
		flex: none;
		min-width: 96px;
		font: 500 11px/1 var(--rb-data);
		font-variant-numeric: tabular-nums;
		color: var(--st-muted);
	}

	.scrub {
		flex: 1 1 auto;
		min-width: 80px;
		accent-color: var(--st-accent);
	}

	.volume {
		flex: none;
		width: 84px;
		accent-color: var(--st-muted);
	}

	/* Full screen: the picture takes the screen; the name and the bar stay small at its edges. */
	.sheet.full header {
		height: 32px;
	}
</style>
