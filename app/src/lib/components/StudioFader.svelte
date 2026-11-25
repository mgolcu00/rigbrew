<script lang="ts">
	import { dbLabel, dbOf, faderToGain, gainOf, gainToFader } from '#lib/studio-edit.ts';

	// A fader: a cap on a groove, even in decibels between its marks. Drag it (Shift for fine
	// steps), turn the wheel over it, use the arrow keys, double-click for 0 dB.

	let {
		value,
		top = 2,
		vertical = false,
		label,
		disabled = false,
		onstart,
		onchange,
		onend,
		onset
	}: {
		/** The gain: 1 is as it is. */
		value: number;
		/** The highest gain the fader gives. */
		top?: number;
		vertical?: boolean;
		label: string;
		disabled?: boolean;
		/** A drag starts, moves and ends: one step for undo. */
		onstart: () => void;
		onchange: (gain: number) => void;
		onend: () => void;
		/** One change that is a step by itself: a key, the wheel, a double-click. */
		onset: (gain: number) => void;
	} = $props();

	let groove = $state<HTMLDivElement | null>(null);
	let dragging = $state(false);
	let grab = { at: 0, travel: 0 };

	const travel = $derived(gainToFader(Math.min(top, value)));
	const most = $derived(gainToFader(top));

	function down(e: PointerEvent) {
		if (disabled || e.button !== 0 || !groove) return;
		e.preventDefault();
		const box = groove.getBoundingClientRect();
		const span = vertical ? box.height : box.width;
		// A press beside the cap brings it there; on the cap it stays under the pointer.
		const pressed = vertical ? 1 - (e.clientY - box.top) / span : (e.clientX - box.left) / span;
		const onCap = Math.abs(pressed - travel) * span < 9;
		grab = { at: vertical ? e.clientY : e.clientX, travel: onCap ? travel : Math.min(most, Math.max(0, pressed)) };
		dragging = true;
		groove.setPointerCapture(e.pointerId);
		onstart();
		if (!onCap) onchange(faderToGain(grab.travel, top));
	}

	function move(e: PointerEvent) {
		if (!dragging || !groove) return;
		const box = groove.getBoundingClientRect();
		const span = vertical ? box.height : box.width;
		const by = ((vertical ? grab.at - e.clientY : e.clientX - grab.at) / span) * (e.shiftKey ? 0.25 : 1);
		onchange(faderToGain(Math.min(most, Math.max(0, grab.travel + by)), top));
	}

	function up(e: PointerEvent) {
		if (!dragging) return;
		dragging = false;
		groove?.releasePointerCapture(e.pointerId);
		onend();
	}

	/** One step up or down: half a decibel, a tenth with Shift. */
	function nudge(up: boolean, fine: boolean) {
		const db = dbOf(value);
		const by = fine ? 0.1 : 0.5;
		const next = db === -Infinity ? (up ? -60 : -Infinity) : db + (up ? by : -by);
		onset(Math.min(top, next < -60 ? 0 : gainOf(next)));
	}

	function keys(e: KeyboardEvent) {
		if (disabled) return;
		if (['ArrowUp', 'ArrowRight', 'ArrowDown', 'ArrowLeft'].includes(e.key)) {
			e.preventDefault();
			e.stopPropagation();
			nudge(e.key === 'ArrowUp' || e.key === 'ArrowRight', e.shiftKey);
		}
	}

	function wheel(e: WheelEvent) {
		if (disabled || e.ctrlKey || e.metaKey) return;
		e.preventDefault();
		e.stopPropagation();
		nudge(e.deltaY < 0 || e.deltaX > 0, true);
	}
</script>

<div
	class="fader"
	class:vertical
	class:dragging
	class:disabled
	bind:this={groove}
	role="slider"
	tabindex={disabled ? -1 : 0}
	aria-label={label}
	aria-orientation={vertical ? 'vertical' : 'horizontal'}
	aria-valuemin={-60}
	aria-valuemax={Math.round(dbOf(top))}
	aria-valuenow={Math.max(-60, Math.round(dbOf(value) * 10) / 10)}
	aria-valuetext="{dbLabel(value)} dB"
	title="{label}: {dbLabel(value)} dB"
	onpointerdown={down}
	onpointermove={move}
	onpointerup={up}
	onpointercancel={up}
	ondblclick={() => !disabled && onset(Math.min(top, 1))}
	onkeydown={keys}
	onwheel={wheel}
	style:--at="{travel * 100}%"
	style:--unity="{gainToFader(1) * 100}%"
>
	<span class="groove"></span>
	<span class="unity"></span>
	<span class="cap"></span>
</div>

<style>
	.fader {
		position: relative;
		flex: 1 1 auto;
		min-width: 40px;
		height: 16px;
		touch-action: none;
		cursor: default;
		outline: none;
	}

	.fader.vertical {
		flex: none;
		width: 22px;
		min-width: 0;
		height: 100%;
	}

	.groove {
		position: absolute;
		left: 0;
		right: 0;
		top: 50%;
		height: 3px;
		margin-top: -1.5px;
		border-radius: 2px;
		background: var(--st-edge);
		box-shadow: 0 1px 0 rgba(255, 255, 255, 0.06);
	}

	.vertical .groove {
		left: 50%;
		right: auto;
		top: 0;
		bottom: 0;
		width: 4px;
		height: auto;
		margin: 0 0 0 -2px;
	}

	/* A small notch where the fader leaves the sound as it is. */
	.unity {
		position: absolute;
		left: var(--unity);
		top: 3px;
		bottom: 3px;
		width: 1px;
		background: var(--st-faint);
		opacity: 0.6;
	}

	.vertical .unity {
		left: 2px;
		right: 2px;
		top: auto;
		bottom: var(--unity);
		width: auto;
		height: 1px;
	}

	.cap {
		position: absolute;
		left: var(--at);
		top: 50%;
		width: 9px;
		height: 14px;
		margin: -7px 0 0 -4.5px;
		border-radius: 2px;
		background: linear-gradient(90deg, #8d919b, #c9ccd3 45%, #8d919b);
		box-shadow:
			0 0 0 1px var(--st-edge),
			0 1px 2px rgba(0, 0, 0, 0.6);
	}

	.cap::after {
		content: '';
		position: absolute;
		left: 4px;
		top: 2px;
		bottom: 2px;
		width: 1px;
		background: #1b1c20;
	}

	.vertical .cap {
		left: 50%;
		top: auto;
		bottom: var(--at);
		width: 20px;
		height: 30px;
		margin: 0 0 -15px -10px;
		border-radius: 2.5px;
		background: linear-gradient(180deg, #d5d8de, #a2a6b0 46%, #6f737d 54%, #9499a3);
	}

	.vertical .cap::after {
		left: 2px;
		right: 2px;
		top: 14px;
		bottom: auto;
		width: auto;
		height: 2px;
		background: #111214;
	}

	.fader:focus-visible .cap,
	.dragging .cap {
		box-shadow:
			0 0 0 1px var(--st-edge),
			0 0 0 2.5px var(--st-accent);
	}

	.disabled {
		opacity: 0.4;
	}
</style>
