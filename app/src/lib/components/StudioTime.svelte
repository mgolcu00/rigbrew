<script lang="ts">
	import { parseTime, shortTime, snapFrame, timecode } from '#lib/studio-time.ts';

	// A moment or a length as seconds and frames ("2:13"), typed or nudged: arrow up and down move
	// it by a frame (a second with Shift); Enter takes what was typed, Escape leaves it.

	let {
		value,
		label,
		full = false,
		min = 0,
		max = Infinity,
		disabled = false,
		onset
	}: {
		value: number;
		label: string;
		/** Hours, minutes, seconds, frames, instead of the short form. */
		full?: boolean;
		min?: number;
		max?: number;
		disabled?: boolean;
		onset: (seconds: number) => void;
	} = $props();

	let typing = $state<string | null>(null);
	const shown = $derived(full ? timecode(value) : shortTime(value));

	function take() {
		const typed = typing;
		typing = null;
		if (typed === null) return;
		const seconds = parseTime(typed);
		if (seconds === null) return;
		const next = Math.min(max, Math.max(min, snapFrame(seconds)));
		if (Math.abs(next - value) > 1e-6) onset(next);
	}

	function keys(e: KeyboardEvent) {
		e.stopPropagation();
		if (e.key === 'Enter') {
			take();
			(e.currentTarget as HTMLInputElement).blur();
		} else if (e.key === 'Escape') {
			typing = null;
			(e.currentTarget as HTMLInputElement).blur();
		} else if (e.key === 'ArrowUp' || e.key === 'ArrowDown') {
			e.preventDefault();
			typing = null;
			const by = (e.shiftKey ? 1 : 1 / 24) * (e.key === 'ArrowUp' ? 1 : -1);
			onset(Math.min(max, Math.max(min, snapFrame(value + by))));
		}
	}
</script>

<input
	class="time"
	class:full
	value={typing ?? shown}
	{disabled}
	aria-label={label}
	title="{label}. Seconds and frames, 24 frames a second. Arrow keys step by a frame."
	spellcheck="false"
	oninput={(e) => (typing = e.currentTarget.value)}
	onfocus={(e) => e.currentTarget.select()}
	onblur={take}
	onkeydown={keys}
/>

<style>
	.time {
		width: 58px;
		height: 20px;
		padding: 0 5px;
		border: 0;
		border-radius: 2.5px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.06);
		color: var(--st-ink);
		font: 500 11px/20px var(--rb-data);
		font-variant-numeric: tabular-nums;
		text-align: right;
		outline: none;
	}

	.time.full {
		width: 88px;
	}

	.time:focus {
		box-shadow: inset 0 0 0 1px var(--st-accent);
	}

	.time:disabled {
		opacity: 0.45;
	}
</style>
