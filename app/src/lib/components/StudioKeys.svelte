<script lang="ts">
	import { tick } from 'svelte';
	import { GROUPS, findTools } from '#lib/studio-tools.ts';
	import StudioIcon from './StudioIcon.svelte';

	// Keys and tools: every tool of the Studio in its group, with what it does and its key.
	// Typing narrows the list to the tools that hold every word typed.

	let { onclose }: { onclose: () => void } = $props();

	let query = $state('');
	let field = $state<HTMLInputElement | null>(null);
	const found = $derived(findTools(query));
	const groups = $derived(GROUPS.map((g) => ({ name: g, tools: found.filter((x) => x.group === g) })).filter((g) => g.tools.length > 0));

	$effect(() => {
		tick().then(() => field?.focus());
	});

	function key(e: KeyboardEvent) {
		e.stopPropagation();
		if (e.key === 'Escape') {
			e.preventDefault();
			if (query) query = '';
			else onclose();
		}
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="veil" onclick={onclose}>
	<div class="keys" role="dialog" aria-label="Keys and tools" tabindex="-1" onclick={(e) => e.stopPropagation()}>
		<header>
			<h2>Keys and tools</h2>
			<input class="st-in find" bind:this={field} bind:value={query} onkeydown={key} placeholder="Find a tool or a key" aria-label="Find a tool or a key" spellcheck="false" />
			<button class="closer" onclick={onclose} aria-label="Close"><StudioIcon name="close" size={11} /></button>
		</header>
		<div class="cols st-scroll">
			{#each groups as g (g.name)}
				<section>
					<h3>{g.name}</h3>
					<dl>
						{#each g.tools as x (x.id)}
							<div class="tool">
								<dt>{x.name}</dt>
								<dd>{x.what}</dd>
								{#if x.key}<kbd>{x.key}</kbd>{/if}
							</div>
						{/each}
					</dl>
				</section>
			{:else}
				<p class="none">No tool has “{query}”.</p>
			{/each}
		</div>
	</div>
</div>

<style>
	.veil {
		position: absolute;
		inset: 0;
		z-index: 40;
		display: grid;
		place-items: center;
		background: rgba(0, 0, 0, 0.5);
	}

	.keys {
		width: min(980px, calc(100% - 40px));
		max-height: calc(100% - 60px);
		display: flex;
		flex-direction: column;
		border-radius: 6px;
		background: var(--st-raised);
		box-shadow:
			0 0 0 1px #000,
			0 0 0 2px rgba(255, 255, 255, 0.06),
			0 24px 70px rgba(0, 0, 0, 0.7);
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 10px;
		height: 40px;
		padding: 0 6px 0 14px;
		border-bottom: 1px solid var(--st-edge);
	}

	h2 {
		flex: none;
		font: 600 12.5px/1 var(--rb-text);
	}

	.find {
		flex: 1;
		max-width: 320px;
		height: 24px;
		margin-left: auto;
	}

	.closer {
		flex: none;
		width: 24px;
		height: 24px;
		display: grid;
		place-items: center;
		color: var(--st-faint);
	}

	.closer:hover {
		color: var(--st-ink);
	}

	.cols {
		min-height: 0;
		padding: 6px 14px 14px;
		columns: 3 260px;
		column-gap: 24px;
	}

	section {
		break-inside: avoid;
		padding-top: 8px;
	}

	h3 {
		margin: 0 0 4px;
		font: 600 9.5px/1 var(--rb-text);
		letter-spacing: 0.12em;
		text-transform: uppercase;
		color: var(--st-accent);
	}

	dl {
		margin: 0;
	}

	.tool {
		display: grid;
		grid-template-columns: minmax(0, 1fr) auto;
		column-gap: 8px;
		padding: 3px 0;
		border-bottom: 1px solid rgba(255, 255, 255, 0.04);
	}

	dt {
		font: 500 11.5px/1.3 var(--rb-text);
		color: var(--st-ink);
	}

	dd {
		grid-column: 1;
		margin: 0;
		font-size: 10.5px;
		line-height: 1.35;
		color: var(--st-faint);
	}

	kbd {
		grid-column: 2;
		grid-row: 1 / span 2;
		align-self: start;
		justify-self: end;
		padding: 2px 6px;
		border-radius: 3px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
		font: 500 10.5px/1.2 var(--rb-data);
		color: var(--st-ink);
		white-space: nowrap;
	}

	.none {
		padding: 16px 0;
		font-size: 11.5px;
		color: var(--st-faint);
	}
</style>
