<script lang="ts">
	import { onDestroy } from 'svelte';
	import { meterFill } from '#lib/studio-edit.ts';
	import { LOOK } from '#lib/studio-look.ts';
	import { studio } from '#lib/studio.svelte.ts';

	// A level meter for one strip or for the master: left and right, on the same travel as the
	// faders, with a line that holds the highest level for a moment and a mark that stays lit
	// once the level went over full (click it to clear).

	let {
		id,
		vertical = false,
		length = 60,
		thick = 6
	}: {
		/** A strip's id, 'shots', or 'master'. */
		id: string;
		vertical?: boolean;
		/** Pixels along the meter, and across both of its bars. */
		length?: number;
		thick?: number;
	} = $props();

	let canvas = $state<HTMLCanvasElement | null>(null);
	let over = $state(false);
	let shown: [number, number] = [0, 0];
	let held: [number, number] = [0, 0];
	let heldAt: [number, number] = [0, 0];
	let frame = 0;

	function paint(now: number) {
		frame = 0;
		const el = canvas;
		if (!el) return;
		const levels = studio.playing ? studio.audio.levels(id) : ([0, 0] as [number, number]);
		let alive = studio.playing;
		for (const side of [0, 1] as const) {
			const fill = meterFill(levels[side]);
			// Up at once, down slowly.
			shown[side] = fill >= shown[side] ? fill : Math.max(fill, shown[side] - 0.022);
			if (fill >= held[side]) {
				held[side] = fill;
				heldAt[side] = now;
			} else if (now - heldAt[side] > 1100) held[side] = Math.max(shown[side], held[side] - 0.012);
			if (levels[side] >= 1) over = true;
			if (shown[side] > 0.002 || held[side] > 0.002) alive = true;
		}
		const dpr = window.devicePixelRatio || 1;
		const [w, h] = vertical ? [thick, length] : [length, thick];
		if (el.width !== Math.round(w * dpr)) {
			el.width = Math.round(w * dpr);
			el.height = Math.round(h * dpr);
		}
		const g = el.getContext('2d')!;
		g.setTransform(dpr, 0, 0, dpr, 0, 0);
		g.clearRect(0, 0, w, h);
		const bar = (thick - 1) / 2;
		const full = meterFill(1);
		for (const side of [0, 1] as const) {
			const across = side * (bar + 1);
			g.fillStyle = LOOK.edge;
			if (vertical) g.fillRect(across, 0, bar, length);
			else g.fillRect(0, across, length, bar);
			// Green up to -12 dB, yellow to full level, red above it.
			const parts: [number, number, string][] = [
				[0, meterFill(0.25), LOOK.meterLow],
				[meterFill(0.25), full, LOOK.meterMid],
				[full, 1, LOOK.meterHigh]
			];
			for (const [from, to, color] of parts) {
				const upTo = Math.min(to, shown[side]);
				if (upTo <= from) continue;
				g.fillStyle = color;
				if (vertical) g.fillRect(across, length * (1 - upTo), bar, length * (upTo - from));
				else g.fillRect(length * from, across, length * (upTo - from), bar);
			}
			if (held[side] > 0.01) {
				g.fillStyle = held[side] >= full ? LOOK.meterHigh : LOOK.ink;
				if (vertical) g.fillRect(across, Math.round(length * (1 - held[side])), bar, 1);
				else g.fillRect(Math.round(length * held[side]) - 1, across, 1, bar);
			}
		}
		if (alive) frame = requestAnimationFrame(paint);
	}

	// The meter runs while the film plays and until it has fallen back.
	$effect(() => {
		void studio.playing;
		void canvas;
		if (!frame) frame = requestAnimationFrame(paint);
	});

	onDestroy(() => cancelAnimationFrame(frame));
</script>

<span class="meter" class:vertical>
	<button class="over" class:lit={over} onclick={() => (over = false)} title={over ? 'The level went over full. Click to clear.' : 'Lights when the level goes over full'} aria-label="Over full level{over ? ': yes' : ': no'}" tabindex="-1"></button>
	<canvas bind:this={canvas} style:width="{vertical ? thick : length}px" style:height="{vertical ? length : thick}px"></canvas>
</span>

<style>
	.meter {
		flex: none;
		display: inline-flex;
		flex-direction: row-reverse;
		align-items: center;
		gap: 2px;
	}

	.meter.vertical {
		flex-direction: column;
	}

	canvas {
		display: block;
	}

	.over {
		flex: none;
		width: 4px;
		height: 6px;
		border-radius: 1px;
		background: var(--st-edge);
	}

	.vertical .over {
		width: 100%;
		height: 4px;
	}

	.over.lit {
		background: var(--st-fail);
	}
</style>
