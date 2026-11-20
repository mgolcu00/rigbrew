<script lang="ts">
	import type { Snippet } from 'svelte';
	import { gb } from '#lib/format.ts';

	// The ruled track: a memory budget drawn as a ruler. Minor ticks, labelled major ticks,
	// and whatever is seated in it (the children place themselves with left/width in percent
	// of the budget). The bay, the fit bars in Models and the Device sheet all use it, so a
	// gigabyte looks the same everywhere.
	let {
		budget,
		height = 20,
		numbers = true,
		label,
		children
	}: { budget: number; height?: number; numbers?: boolean; label: string; children?: Snippet } = $props();

	let width = $state(0);
	const total = $derived(Math.max(budget / 1024, 0.001));
	const pxPerGb = $derived(width / total);
	/** Smallest step whose ticks are at least `min` pixels apart. */
	const stepFor = (min: number) => [0.25, 0.5, 1, 2, 4, 8, 16, 32, 64, 128].find((s) => s * pxPerGb >= min) ?? 256;
	const minor = $derived(stepFor(5));
	const major = $derived(Math.max(stepFor(54), minor * 2));
	const marks = $derived.by(() => {
		if (!numbers || width === 0) return [];
		const out: number[] = [];
		// Leave the end of the track to its own figure.
		for (let v = major; v < total - major * 0.55; v += major) out.push(v);
		return out;
	});
</script>

<div
	class="track"
	role="img"
	aria-label={label}
	bind:clientWidth={width}
	style:height="{height}px"
	style:--minor="{(minor / total) * 100}%"
	style:--major="{(major / total) * 100}%"
>
	<div class="ticks" aria-hidden="true"></div>
	{#each marks as mark (mark)}
		<span class="mark num" style:left="{(mark / total) * 100}%" aria-hidden="true">{mark}</span>
	{/each}
	{#if numbers}<span class="end num" aria-hidden="true">{gb(budget)} GB</span>{/if}
	{@render children?.()}
</div>

<style>
	.track {
		position: relative;
		min-width: 0;
		border-bottom: 1px solid var(--muted);
	}

	/* The ends of the budget. */
	.track::before,
	.track::after {
		content: '';
		position: absolute;
		bottom: 0;
		width: 1px;
		height: 100%;
		background: var(--muted);
	}

	.track::before {
		left: 0;
	}

	.track::after {
		right: 0;
	}

	.ticks {
		position: absolute;
		inset: 0;
		background-image:
			linear-gradient(to right, var(--muted) 1px, transparent 1px), linear-gradient(to right, var(--line) 1px, transparent 1px);
		background-size:
			var(--major) 7px,
			var(--minor) 4px;
		background-position: left bottom;
		background-repeat: repeat-x;
	}

	.mark,
	.end {
		position: absolute;
		bottom: 7px;
		font-size: 10px;
		line-height: 10px;
		color: var(--muted);
		pointer-events: none;
	}

	.mark {
		padding-left: 3px;
	}

	.end {
		right: 4px;
	}
</style>
