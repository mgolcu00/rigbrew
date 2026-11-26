<script lang="ts">
	import { panLabel } from '#lib/studio-edit.ts';
	import { tag } from '#lib/studio-tags.ts';

	// A pan knob: drag up or to the right for right, down or left for left (Shift for fine
	// steps), arrow keys, the wheel, double-click for the middle.

	let {
		value,
		size = 20,
		label,
		onstart,
		onchange,
		onend,
		onset
	}: {
		/** -1 is all left, 1 all right. */
		value: number;
		size?: number;
		label: string;
		onstart: () => void;
		onchange: (pan: number) => void;
		onend: () => void;
		onset: (pan: number) => void;
	} = $props();

	let el = $state<HTMLDivElement | null>(null);
	let dragging = $state(false);
	let grab = { x: 0, y: 0, pan: 0 };

	const hold = (pan: number) => {
		const p = Math.min(1, Math.max(-1, pan));
		// The middle holds a little, so it is easy to find again.
		return Math.abs(p) < 0.03 ? 0 : Math.round(p * 100) / 100;
	};

	function down(e: PointerEvent) {
		if (e.button !== 0) return;
		e.preventDefault();
		grab = { x: e.clientX, y: e.clientY, pan: value };
		dragging = true;
		el?.setPointerCapture(e.pointerId);
		onstart();
	}

	function move(e: PointerEvent) {
		if (!dragging) return;
		const by = (e.clientX - grab.x + (grab.y - e.clientY)) / (e.shiftKey ? 400 : 90);
		onchange(hold(grab.pan + by));
	}

	function up(e: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		el?.releasePointerCapture(e.pointerId);
		onend();
	}

	function keys(e: KeyboardEvent) {
		const by = e.key === 'ArrowUp' || e.key === 'ArrowRight' ? 1 : e.key === 'ArrowDown' || e.key === 'ArrowLeft' ? -1 : 0;
		if (!by) return;
		e.preventDefault();
		e.stopPropagation();
		onset(Math.min(1, Math.max(-1, Math.round((value + by * (e.shiftKey ? 0.01 : 0.05)) * 100) / 100)));
	}

	function wheel(e: WheelEvent) {
		if (e.ctrlKey || e.metaKey) return;
		e.preventDefault();
		e.stopPropagation();
		onset(Math.min(1, Math.max(-1, Math.round((value + (e.deltaY < 0 ? 0.02 : -0.02)) * 100) / 100)));
	}

	// The pointer sweeps 270 degrees; the arc is drawn from the top toward where it points.
	const angle = $derived(value * 135);
	const arc = $derived.by(() => {
		const r = 7.2;
		const a = ((angle - 90) * Math.PI) / 180;
		const x = 10 + r * Math.cos(a);
		const y = 10 + r * Math.sin(a);
		return `M10 ${10 - r}A${r} ${r} 0 0 ${value >= 0 ? 1 : 0} ${x.toFixed(2)} ${y.toFixed(2)}`;
	});
</script>

<div
	class="knob"
	class:dragging
	bind:this={el}
	role="slider"
	tabindex="0"
	aria-label={label}
	aria-valuemin={-100}
	aria-valuemax={100}
	aria-valuenow={Math.round(value * 100)}
	aria-valuetext={panLabel(value)}
	use:tag={{ name: `${label}: ${panLabel(value)}`, what: 'Drag up or down to turn; double-click for the middle.' }}
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={up}
	ondblclick={() => onset(0)}
	onkeydown={keys}
	onwheel={wheel}
	style:width="{size}px"
	style:height="{size}px"
>
	<svg viewBox="0 0 20 20" width={size} height={size} aria-hidden="true">
		<path d="M4.9 15.1A7.2 7.2 0 1 1 15.1 15.1" class="ring" />
		{#if value !== 0}<path d={arc} class="arc" />{/if}
		<circle cx="10" cy="10" r="5.2" class="body" />
		<line x1="10" y1="10" x2="10" y2="5.6" class="mark" transform="rotate({angle} 10 10)" />
	</svg>
</div>

<style>
	.knob {
		flex: none;
		touch-action: none;
		outline: none;
		border-radius: 50%;
	}

	.ring {
		fill: none;
		stroke: var(--st-edge);
		stroke-width: 2;
		stroke-linecap: round;
	}

	.arc {
		fill: none;
		stroke: var(--st-accent);
		stroke-width: 2;
		stroke-linecap: round;
	}

	.body {
		fill: #3a3d45;
		stroke: #0a0a0b;
		stroke-width: 1;
	}

	.mark {
		stroke: #e3e5e9;
		stroke-width: 1.5;
		stroke-linecap: round;
	}

	.knob:focus-visible .body,
	.dragging .body {
		stroke: var(--st-accent);
	}
</style>
