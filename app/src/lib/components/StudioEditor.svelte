<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import { dbLabel, regionEnd, regionSpan, trimRegionEnd, trimRegionStart, trimShot, type DocRegion } from '#lib/studio-edit.ts';
	import { LOOK, darker, fileName, lighter } from '#lib/studio-look.ts';
	import { tag } from '#lib/studio-tags.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { rulerLabel, rulerSteps, shortTime, snapFrame, zoomSpanAround, zoomedSpan } from '#lib/studio-time.ts';
	import { hasClip, shotSeconds } from '#lib/studio.ts';
	import { mod } from '#lib/words.ts';

	// The Editor: what is selected, as wide as the panel. A shot is its real frames as a filmstrip
	// over its sound, with a handle at each end to trim it. A sound region is its file's waveform
	// with handles for where it starts and stops in the file, and for its fades.

	let wrap = $state<HTMLDivElement | null>(null);
	let canvas = $state<HTMLCanvasElement | null>(null);
	let line = $state<HTMLDivElement | null>(null);
	let width = $state(0);
	let height = $state(0);
	let fonts = { text: 'sans-serif', data: 'monospace' };
	let tip = $state('');

	const PAD = 12;
	const TOP = 20;

	const shot = $derived(studio.shotsSel.length === 1 ? studio.shot(studio.shotsSel[0]) : undefined);
	const found = $derived(studio.regionsSel.length === 1 ? studio.region(studio.regionsSel[0]) : null);
	const part = $derived(shot ? studio.plan.parts.find((p) => p.shot === shot.id) : undefined);

	/** What the editor shows from left to right, in seconds of the clip or of the sound file. */
	let frozen = $state<{ from: number; to: number } | null>(null);
	/** The Editor's own zoom: 1 shows all of it, more shows a part, from `panFrom` on. */
	let ez = $state(1);
	let panFrom = $state(0);
	const span = $derived.by(() => {
		// While a handle is dragged the scale stands still.
		if (frozen) return frozen;
		return zoomedSpan(whole, ez, panFrom);
	});
	const whole = $derived.by(() => {
		void studio.drawn;
		if (shot) return { from: 0, to: shotSeconds(shot) };
		if (found) {
			const file = studio.fileSeconds(found.region.file) ?? 0;
			// A little of the file on both sides of the region, all of it when it is short.
			const r = found.region;
			const room = Math.max(r.length * 0.35, 0.5);
			const from = Math.max(0, r.offset - room);
			const to = Math.max(r.offset + r.length + room, from + 1);
			return { from, to: file > 0 ? Math.min(Math.max(file, r.offset + r.length), to) : to };
		}
		return { from: 0, to: 1 };
	});
	const pps = $derived((width - PAD * 2) / Math.max(0.05, span.to - span.from));

	// Another shot or region: all of it again.
	$effect(() => {
		void shot?.id;
		void found?.region.id;
		ez = 1;
		panFrom = 0;
	});

	function zoomBy(factor: number, x = 0.5) {
		const next = zoomSpanAround(whole, span, factor, x);
		ez = next.zoom;
		panFrom = next.from;
	}

	/** Pinch, or Ctrl or Cmd and the wheel: zoom around the pointer. Sideways, or Shift and the wheel: move along. */
	function wheel(e: WheelEvent) {
		if (!shot && !found) return;
		if (e.ctrlKey || e.metaKey) {
			e.preventDefault();
			const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
			zoomBy(Math.exp(-e.deltaY * 0.01), (e.clientX - box.left - PAD) / Math.max(1, width - PAD * 2));
			return;
		}
		const along = Math.abs(e.deltaX) > Math.abs(e.deltaY) ? e.deltaX : e.shiftKey ? e.deltaY : 0;
		if (along === 0 || ez <= 1) return;
		e.preventDefault();
		panFrom = zoomedSpan(whole, ez, span.from + along / pps).from;
	}
	const xOf = (seconds: number) => PAD + (seconds - span.from) * pps;
	const tOf = (x: number) => span.from + (x - PAD) / pps;

	// --- the real frames of a shot -----------------------------------------------------------------

	const strips = new Map<string, HTMLCanvasElement[]>();
	let stripsDrawn = $state(0);
	let grabbing = '';
	/** Reads pictures out of a clip, evenly spread, by seeking a hidden video. */
	async function frames(key: string, shotId: string, seconds: number, count: number) {
		if (strips.has(key) || grabbing === key) return;
		grabbing = key;
		const url = await studio.clipOf(shotId);
		if (!url) return void (grabbing = '');
		const video = document.createElement('video');
		video.muted = true;
		video.preload = 'auto';
		video.src = url;
		const out: HTMLCanvasElement[] = [];
		try {
			await new Promise<void>((done, fail) => {
				video.onloadeddata = () => done();
				video.onerror = () => fail(new Error('clip'));
			});
			for (let i = 0; i < count; i++) {
				const at = Math.min(Math.max(0, seconds - 0.05), ((i + 0.5) / count) * seconds);
				await new Promise<void>((done) => {
					video.onseeked = () => done();
					video.currentTime = at;
				});
				const c = document.createElement('canvas');
				c.height = 120;
				c.width = Math.round((120 * video.videoWidth) / Math.max(1, video.videoHeight));
				c.getContext('2d')!.drawImage(video, 0, 0, c.width, c.height);
				out.push(c);
				strips.set(key, [...out]);
				stripsDrawn++;
				if (grabbing !== key) break;
			}
		} catch {
			// The poster strip stays.
		}
		video.removeAttribute('src');
		video.load();
		if (grabbing === key) grabbing = '';
	}

	// --- drawing ------------------------------------------------------------------------------------

	function wave(g: CanvasRenderingContext2D, pairs: Int8Array, rate: number, x0: number, x1: number, mid: number, half: number, gain: number) {
		const a = Math.max(0, Math.floor(x0));
		const b = Math.min(width, Math.ceil(x1));
		if (b <= a || pairs.length === 0) return;
		const tops: number[] = [];
		const bottoms: number[] = [];
		const count = pairs.length / 2;
		for (let px = a; px < b; px++) {
			const i0 = Math.floor(tOf(px) * rate);
			const i1 = Math.max(i0 + 1, Math.floor(tOf(px + 1) * rate));
			let lo = 0;
			let hi = 0;
			for (let i = Math.max(0, i0); i < Math.min(count, i1); i++) {
				if (pairs[i * 2] < lo) lo = pairs[i * 2];
				if (pairs[i * 2 + 1] > hi) hi = pairs[i * 2 + 1];
			}
			tops.push(mid - Math.min(1, (hi / 127) * gain) * half - 0.5);
			bottoms.push(mid - Math.max(-1, (lo / 127) * gain) * half + 0.5);
		}
		g.beginPath();
		g.moveTo(a, tops[0]);
		for (let i = 1; i < tops.length; i++) g.lineTo(a + i, tops[i]);
		for (let i = bottoms.length - 1; i >= 0; i--) g.lineTo(a + i, bottoms[i]);
		g.closePath();
		g.fill();
	}

	function handle(g: CanvasRenderingContext2D, x: number, y: number, h: number, left: boolean) {
		g.fillStyle = '#ffffff';
		g.fillRect(Math.round(x) - (left ? 0 : 2), y, 2, h);
		g.beginPath();
		g.roundRect(Math.round(x) - (left ? 0 : 8), y + h / 2 - 12, 8, 24, 2);
		g.fill();
		g.fillStyle = '#1b1c20';
		g.fillRect(Math.round(x) + (left ? 3 : -5), y + h / 2 - 6, 1.5, 12);
	}

	function draw() {
		const el = canvas;
		if (!el || width <= 0 || height <= 0) return;
		const dpr = window.devicePixelRatio || 1;
		if (el.width !== Math.round(width * dpr) || el.height !== Math.round(height * dpr)) {
			el.width = Math.round(width * dpr);
			el.height = Math.round(height * dpr);
		}
		const g = el.getContext('2d')!;
		g.setTransform(dpr, 0, 0, dpr, 0, 0);
		g.fillStyle = LOOK.bg;
		g.fillRect(0, 0, width, height);
		if (!shot && !found) return;
		// A ruler of the clip or the file.
		g.fillStyle = LOOK.raised;
		g.fillRect(0, 0, width, TOP);
		const steps = rulerSteps(pps, 70);
		g.font = `500 9.5px ${fonts.data}`;
		g.textBaseline = 'alphabetic';
		const each = Math.round(steps.major / steps.minor);
		for (let n = Math.ceil(span.from / steps.minor); n * steps.minor <= span.to + 1e-6; n++) {
			const x = Math.round(xOf(n * steps.minor));
			const major = n % each === 0;
			g.fillStyle = major ? LOOK.faint : LOOK.line;
			g.fillRect(x, major ? 4 : TOP - 5, 1, major ? TOP - 4 : 5);
			if (major) {
				g.fillStyle = LOOK.muted;
				g.fillText(rulerLabel(n * steps.minor, steps.frames), x + 4, 13);
			}
		}
		const y = TOP + 6;
		const h = height - y - 8;
		if (shot && hasClip(shot)) {
			const whole = shotSeconds(shot);
			const x0 = xOf(0);
			const x1 = xOf(whole);
			const filmH = Math.round(h * 0.62);
			// The real frames, as many as fit; the poster until they are read.
			const poster = studio.posters.get(studio.posterKey(shot));
			const ratio = poster ? poster.naturalWidth / Math.max(1, poster.naturalHeight) : 5 / 3;
			const tw = filmH * ratio;
			const count = Math.max(1, Math.min(48, Math.round((x1 - x0) / tw)));
			const key = `${studio.posterKey(shot)}#${count}`;
			const real = strips.get(key);
			if (!real || real.length < count) frames(key, shot.id, whole, count);
			const each = (x1 - x0) / count;
			g.save();
			g.beginPath();
			g.roundRect(x0, y, x1 - x0, filmH, 3);
			g.clip();
			g.fillStyle = '#000';
			g.fillRect(x0, y, x1 - x0, filmH);
			for (let i = 0; i < count; i++) {
				const picture = real?.[i] ?? poster;
				if (picture) {
					const pw = 'naturalWidth' in picture ? picture.naturalWidth : picture.width;
					const ph = 'naturalHeight' in picture ? picture.naturalHeight : picture.height;
					// Each frame fills its cell, cut at the sides when the cell is narrower.
					const sw = Math.min(pw, (each / filmH) * ph);
					g.globalAlpha = real?.[i] ? 1 : 0.35;
					g.drawImage(picture, (pw - sw) / 2, 0, sw, ph, x0 + i * each, y, each, filmH);
					g.globalAlpha = 1;
				}
				g.fillStyle = 'rgba(0,0,0,0.6)';
				g.fillRect(Math.round(x0 + i * each), y, 1, filmH);
			}
			g.restore();
			// Its sound under it.
			const sy = y + filmH + 5;
			const sh = h - filmH - 5;
			g.fillStyle = darker('#1f4f5c', 0.25);
			g.beginPath();
			g.roundRect(x0, sy, x1 - x0, sh, 3);
			g.fill();
			const peaks = studio.peaks.get(studio.peaksKey(shot));
			if (peaks) {
				g.fillStyle = shot.mute ? 'rgba(255,255,255,0.25)' : 'rgba(170,232,240,0.82)';
				wave(g, peaks.pairs, peaks.rate, x0, x1, sy + sh / 2, sh / 2 - 2, shot.mute ? 0.3 : (shot.volume ?? 1));
			}
			// What the trims cut away is dimmed; a handle stands at each end of what plays.
			const a = xOf(shot.trim_start ?? 0);
			const b = xOf(whole - (shot.trim_end ?? 0));
			g.fillStyle = 'rgba(8,9,11,0.72)';
			g.fillRect(x0, y, a - x0, h);
			g.fillRect(b, y, x1 - b, h);
			handle(g, a, y, h, true);
			handle(g, b, y, h, false);
		} else if (shot) {
			g.font = `500 12px ${fonts.text}`;
			g.fillStyle = LOOK.faint;
			g.fillText('This shot is not made yet. Its frames and its sound show here once it is.', PAD, y + 24);
		} else if (found) {
			const r = found.region;
			const color = found.track.color;
			const peaks = studio.peaks.get(`file:${r.file}`);
			const fileEnd = peaks && peaks.seconds > 0 ? peaks.seconds : span.to;
			// The whole file, quietly; the region on it, in its colour.
			g.fillStyle = '#202227';
			g.beginPath();
			g.roundRect(xOf(0), y, xOf(fileEnd) - xOf(0), h, 3);
			g.fill();
			if (peaks) {
				g.fillStyle = 'rgba(255,255,255,0.16)';
				wave(g, peaks.pairs, peaks.rate, xOf(0), xOf(fileEnd), y + h / 2, h / 2 - 4, Math.min(2, r.gain));
			}
			const a = xOf(r.offset);
			const b = xOf(r.offset + r.length);
			const fill = g.createLinearGradient(0, y, 0, y + h);
			fill.addColorStop(0, lighter(color, 0.1));
			fill.addColorStop(1, darker(color, 0.2));
			g.fillStyle = fill;
			g.beginPath();
			g.roundRect(a, y, b - a, h, 3);
			g.fill();
			if (peaks) {
				g.save();
				g.beginPath();
				g.rect(a, y, b - a, h);
				g.clip();
				g.fillStyle = 'rgba(6,8,12,0.7)';
				wave(g, peaks.pairs, peaks.rate, a, b, y + h / 2, h / 2 - 4, Math.min(2, r.gain));
				g.restore();
			}
			// Fades, as the lines the level follows.
			g.strokeStyle = '#ffffff';
			g.fillStyle = 'rgba(8,9,11,0.45)';
			g.lineWidth = 1.25;
			const fi = Math.min(r.fade_in, r.length);
			const fo = Math.min(r.fade_out, r.length);
			if (r.loops <= 1) {
				if (fi > 0) {
					g.beginPath();
					g.moveTo(a, y);
					g.lineTo(a + fi * pps, y);
					g.lineTo(a, y + h);
					g.fill();
					g.beginPath();
					g.moveTo(a, y + h);
					g.lineTo(a + fi * pps, y);
					g.stroke();
				}
				if (fo > 0) {
					g.beginPath();
					g.moveTo(b, y);
					g.lineTo(b - fo * pps, y);
					g.lineTo(b, y + h);
					g.fill();
					g.beginPath();
					g.moveTo(b, y + h);
					g.lineTo(b - fo * pps, y);
					g.stroke();
				}
				for (const fx of [a + fi * pps, b - fo * pps]) {
					g.beginPath();
					g.arc(fx, y + 1, 4, 0, Math.PI * 2);
					g.fillStyle = '#ffffff';
					g.fill();
					g.strokeStyle = 'rgba(0,0,0,0.7)';
					g.stroke();
					g.strokeStyle = '#ffffff';
					g.fillStyle = 'rgba(8,9,11,0.45)';
				}
			}
			handle(g, a, y, h, true);
			handle(g, b, y, h, false);
		}
		place();
	}

	/** Where the playhead is on what the Editor shows, when it is inside it. */
	function playX(): number | null {
		if (shot && hasClip(shot) && part) {
			const into = studio.time - part.start;
			return into >= 0 && into <= part.length ? xOf(part.from + into) : null;
		}
		if (!shot && found) {
			const r = found.region;
			const into = studio.time - r.start;
			return into >= 0 && into <= regionSpan(r) ? xOf(r.offset + (into % r.length)) : null;
		}
		return null;
	}

	/** The playhead is a line of its own over the canvas: while the film plays only it moves, and nothing is drawn again. */
	function place() {
		if (!line) return;
		const x = playX();
		line.style.visibility = x === null || x < 0 || x > width ? 'hidden' : 'visible';
		if (x !== null) line.style.transform = `translateX(${Math.round(x)}px)`;
	}

	$effect(() => {
		void stripsDrawn;
		void studio.drawn;
		void studio.jumps;
		void width;
		void height;
		void span;
		draw();
	});
	const stopFrames = studio.onFrame(() => (shot || found) && place());

	// --- handles --------------------------------------------------------------------------------------

	type Grip = 'in' | 'out' | 'fadein' | 'fadeout' | '';
	let grip: Grip = '';
	let orig: { shot?: { trim_start: number; trim_end: number }; region?: DocRegion; x: number } | null = null;

	function gripAt(x: number, y: number): Grip {
		if (shot && hasClip(shot)) {
			const a = xOf(shot.trim_start ?? 0);
			const b = xOf(shotSeconds(shot) - (shot.trim_end ?? 0));
			if (Math.abs(x - a - 4) <= 7) return 'in';
			if (Math.abs(x - b + 4) <= 7) return 'out';
		} else if (found) {
			const r = found.region;
			const a = xOf(r.offset);
			const b = xOf(r.offset + r.length);
			if (r.loops <= 1 && y < TOP + 18) {
				if (Math.abs(x - (a + r.fade_in * pps)) <= 7) return 'fadein';
				if (Math.abs(x - (b - r.fade_out * pps)) <= 7) return 'fadeout';
			}
			if (Math.abs(x - a - 4) <= 7 && r.loops <= 1) return 'in';
			if (Math.abs(x - b + 4) <= 7 && r.loops <= 1) return 'out';
		}
		return '';
	}

	const local = (e: PointerEvent) => {
		const box = canvas!.getBoundingClientRect();
		return { x: e.clientX - box.left, y: e.clientY - box.top };
	};

	function down(e: PointerEvent) {
		if (e.button !== 0 || !canvas) return;
		const { x, y } = local(e);
		grip = gripAt(x, y);
		canvas.setPointerCapture(e.pointerId);
		if (!grip) {
			// A click anywhere else moves the playhead there.
			seekTo(x);
			return;
		}
		studio.begin();
		frozen = { ...span };
		orig = { x, shot: shot ? { trim_start: shot.trim_start ?? 0, trim_end: shot.trim_end ?? 0 } : undefined, region: found ? { ...found.region } : undefined };
	}

	function seekTo(x: number) {
		const t = tOf(x);
		if (shot && part) studio.seek(part.start + Math.min(part.length, Math.max(0, snapFrame(t) - part.from)));
		else if (found) studio.seek(found.region.start + Math.min(regionSpan(found.region), Math.max(0, snapFrame(t - found.region.offset))));
	}

	function move(e: PointerEvent) {
		if (!canvas) return;
		const { x, y } = local(e);
		if (e.buttons === 0) {
			const g = gripAt(x, y);
			canvas.style.cursor = g === 'in' ? 'w-resize' : g === 'out' ? 'e-resize' : g ? 'ew-resize' : 'default';
			return;
		}
		if (!grip) return seekTo(x);
		if (!orig) return;
		const delta = (x - orig.x) / pps;
		if (shot && orig.shot) {
			const next = trimShot(orig.shot, shotSeconds(shot), grip === 'in' ? 'start' : 'end', delta);
			studio.live(() => Object.assign(studio.docShot(shot.id) ?? {}, next));
			tip = `${grip === 'in' ? `Start cut ${shortTime(next.trim_start)}` : `End cut ${shortTime(next.trim_end)}`}  ·  Length ${shortTime(shotSeconds(shot) - next.trim_start - next.trim_end)}`;
		} else if (found && orig.region) {
			const o = orig.region;
			const free = e.altKey;
			const at = (seconds: number) => (free ? seconds : snapFrame(seconds));
			let next = o;
			if (grip === 'in') next = trimRegionStart(o, at(o.start + delta));
			else if (grip === 'out') next = trimRegionEnd(o, at(o.start + o.length + delta), studio.fileSeconds(o.file));
			else if (grip === 'fadein') next = { ...o, fade_in: Math.min(Math.max(0, at(o.fade_in + delta)), o.length - o.fade_out) };
			else if (grip === 'fadeout') next = { ...o, fade_out: Math.min(Math.max(0, at(o.fade_out - delta)), o.length - o.fade_in) };
			studio.live((doc) => {
				for (const t of doc.tracks) {
					const i = t.regions.findIndex((r) => r.id === o.id);
					if (i >= 0) t.regions[i] = next;
				}
			});
			tip = grip === 'fadein' ? `Fade in ${shortTime(next.fade_in)}` : grip === 'fadeout' ? `Fade out ${shortTime(next.fade_out)}` : `In ${shortTime(next.offset)}  ·  Out ${shortTime(next.offset + next.length)}  ·  Length ${shortTime(next.length)}`;
		}
		draw();
	}

	function up(e: PointerEvent) {
		canvas?.releasePointerCapture(e.pointerId);
		if (grip) studio.commit(grip === 'fadein' || grip === 'fadeout' ? 'Fade' : shot ? 'Trim shot' : 'Trim region');
		grip = '';
		orig = null;
		frozen = null;
		tip = '';
	}

	let observer: ResizeObserver | undefined;
	onMount(() => {
		if (wrap) {
			const style = getComputedStyle(wrap);
			fonts = { text: style.fontFamily || 'sans-serif', data: style.getPropertyValue('--rb-data').trim() || 'monospace' };
			observer = new ResizeObserver(() => {
				width = wrap!.clientWidth;
				height = wrap!.clientHeight;
			});
			observer.observe(wrap);
		}
	});
	onDestroy(() => {
		stopFrames();
		observer?.disconnect();
		grabbing = '';
	});
</script>

<div class="editor" role="region" aria-label="Editor">
	<div class="what">
		{#if shot}
			<span class="kind">Shot {studio.shots.findIndex((s) => s.id === shot.id) + 1}</span>
			{#if hasClip(shot)}
				<span class="fact">Plays {shortTime(shotSeconds(shot) - (shot.trim_start ?? 0) - (shot.trim_end ?? 0))} of {shortTime(shotSeconds(shot))}</span>
				<span class="fact">Sound {shot.mute ? 'muted' : `${dbLabel(shot.volume ?? 1)} dB`}</span>
				<span class="fact dim">Drag the handles to trim. Click to move the playhead.</span>
			{/if}
		{:else if found}
			<span class="kind">{fileName(found.region.file)}</span>
			<span class="fact">In {shortTime(found.region.offset)}</span>
			<span class="fact">Out {shortTime(found.region.offset + found.region.length)}</span>
			<span class="fact">Ends {shortTime(regionEnd(found.region))}</span>
			<span class="fact">{dbLabel(found.region.gain)} dB</span>
			{#if found.region.loops > 1}<span class="fact">Plays {found.region.loops} times</span>{/if}
		{:else}
			<span class="fact dim">Select one shot or one sound region to see it here.</span>
		{/if}
		<span class="grow"></span>
		{#if tip}<span class="tip">{tip}</span>{/if}
		{#if shot || found}
			<span class="ezoom" role="group" aria-label="Zoom of the Editor">
				<button onclick={() => zoomBy(1 / 1.6)} disabled={ez <= 1} aria-label="Zoom out" use:tag={{ name: 'Zoom out', what: 'Shows more of it. Pinch, or Ctrl and the wheel, too.' }}>−</button>
				<button class="fit" onclick={() => ((ez = 1), (panFrom = 0))} disabled={ez <= 1} aria-label="Show all of it" use:tag={{ name: 'Show all of it', what: 'The whole shot or region across the Editor.' }}>{ez > 1 ? `${ez < 10 ? ez.toFixed(1) : Math.round(ez)}×` : 'All'}</button>
				<button onclick={() => zoomBy(1.6)} disabled={ez >= 64} aria-label="Zoom in" use:tag={{ name: 'Zoom in', what: `Shows less of it, larger. Pinch, or ${mod} and the wheel, too.` }}>+</button>
			</span>
		{/if}
	</div>
	<div class="stage" bind:this={wrap} onwheel={wheel}>
		<canvas bind:this={canvas} style:width="{width}px" style:height="{height}px" onpointerdown={down} onpointermove={move} onpointerup={up} onpointercancel={up} aria-label="The selected shot or region, enlarged" use:tag={shot ? 'trim' : found ? { id: 'fade', name: 'Trim and fade', what: 'Drag an end to trim; drag a top corner to fade.' } : { name: 'Editor', what: 'Select one shot or one sound region to see it here.' }}></canvas>
		<div class="eplay" bind:this={line} style:top="{TOP}px" style:height="{Math.max(0, height - TOP)}px" aria-hidden="true"></div>
	</div>
</div>

<style>
	.ezoom {
		flex: none;
		display: inline-flex;
		margin-left: 8px;
		border-radius: 3px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
	}

	.ezoom button {
		min-width: 18px;
		height: 16px;
		padding: 0 4px;
		font: 500 10.5px/16px var(--rb-data);
		color: var(--st-muted);
	}

	.ezoom button.fit {
		min-width: 34px;
	}

	.ezoom button:hover:not(:disabled) {
		color: var(--st-ink);
	}

	.ezoom button:disabled {
		opacity: 0.45;
	}

	.editor {
		height: 100%;
		min-height: 0;
		display: flex;
		flex-direction: column;
		background: var(--st-bg);
	}

	.what {
		flex: none;
		height: 22px;
		display: flex;
		align-items: center;
		gap: 12px;
		padding: 0 10px;
		background: var(--st-panel);
		border-bottom: 1px solid var(--st-edge);
		white-space: nowrap;
		overflow: hidden;
	}

	.kind {
		font-size: 11.5px;
		font-weight: 700;
		color: var(--st-ink);
	}

	.fact {
		font: 500 10.5px/1 var(--rb-data);
		font-variant-numeric: tabular-nums;
		color: var(--st-muted);
	}

	.fact.dim {
		font-family: var(--rb-text);
		color: var(--st-faint);
	}

	.grow {
		flex: 1;
	}

	.tip {
		font: 500 10.5px/1 var(--rb-data);
		color: var(--st-cycle);
	}

	.stage {
		position: relative;
		flex: 1 1 0;
		min-height: 0;
		overflow: hidden;
	}

	.eplay {
		position: absolute;
		left: 0;
		width: 1px;
		background: var(--st-playhead);
		pointer-events: none;
		visibility: hidden;
		will-change: transform;
	}

	canvas {
		display: block;
		touch-action: none;
	}
</style>
