<script lang="ts">
	import { ux } from '#lib/experience.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { articleById } from '#lib/help.ts';

	// A small "?" next to a term: opens the help article that explains it. Pro hides them.
	let { topic }: { topic: string } = $props();
	const title = $derived(articleById(topic)?.title ?? 'Help');
</script>

{#if ux.flags.hints}
	<button
		class="hint"
		type="button"
		onclick={(e) => {
			e.stopPropagation();
			guide.openHelp(topic, 'hint');
		}}
		title="Help: {title}"
		aria-label="Help: {title}">?</button
	>
{/if}

<style>
	.hint {
		flex: none;
		display: inline-grid;
		place-items: center;
		width: 15px;
		height: 15px;
		border-radius: 50%;
		box-shadow: inset 0 0 0 1px var(--line);
		font: 600 10px/1 var(--rb-text);
		color: var(--muted);
		vertical-align: 1px;
		transition:
			color var(--quick),
			box-shadow var(--quick);
	}

	.hint:hover {
		color: var(--accent);
		box-shadow: inset 0 0 0 1px var(--accent);
	}
</style>
