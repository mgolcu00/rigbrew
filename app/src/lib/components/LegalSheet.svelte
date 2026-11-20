<script lang="ts">
	import { tick } from 'svelte';
	import { openExternal } from '#lib/api.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { legal, legalLink } from '#lib/legal.ts';
	import { renderMarkdown } from '#lib/markdown.ts';

	// The Privacy Policy or the Terms of Use, read inside the app in the same kind of sheet as
	// Help. The text comes from docs/legal (src/lib/legal.ts); links between the two stay here,
	// web links open in the system browser.
	const doc = $derived(guide.legal ? legal[guide.legal] : null);
	let reader = $state<HTMLElement>();
	let closer = $state<HTMLButtonElement>();

	$effect(() => {
		guide.legal;
		tick().then(() => {
			reader?.scrollTo({ top: 0 });
			closer?.focus();
		});
	});

	function onclick(event: MouseEvent) {
		const link = (event.target as HTMLElement).closest('a');
		if (!link) return;
		event.preventDefault();
		const other = legalLink(link.getAttribute('href') ?? '');
		if (other) guide.legal = other;
		else openExternal(link.href);
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && guide.legal) {
			event.preventDefault();
			event.stopPropagation();
			guide.legal = null;
		}
	}
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#if doc}
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="veil" onclick={() => (guide.legal = null)}>
		<div class="sheet" role="dialog" tabindex="-1" aria-modal="true" aria-label={doc.title} onclick={(e) => e.stopPropagation()}>
			<header>
				<h1>{doc.title}</h1>
				<span class="grow"></span>
				<button class="btn quiet" class:on={guide.legal === 'privacy'} onclick={() => (guide.legal = 'privacy')}>Privacy Policy</button>
				<button class="btn quiet" class:on={guide.legal === 'terms'} onclick={() => (guide.legal = 'terms')}>Terms of Use</button>
				<button class="btn" bind:this={closer} onclick={() => (guide.legal = null)}>Close</button>
			</header>
			<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_noninteractive_element_interactions -->
			<article class="reader scroll selectable" bind:this={reader} tabindex="-1" {onclick}>{@html renderMarkdown(doc.text, false)}</article>
		</div>
	</div>
{/if}

<style>
	.veil {
		position: fixed;
		inset: 0;
		z-index: 70;
		display: flex;
		justify-content: center;
		align-items: flex-start;
		padding: 40px 16px 16px;
		background: color-mix(in srgb, var(--rb-board) 62%, transparent);
	}

	.sheet {
		width: min(820px, 100%);
		height: min(720px, 100%);
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 6px;
		background: var(--bg);
		box-shadow: 0 18px 50px -14px var(--shade);
		overflow: hidden;
		animation: appear 0.14s var(--ease) both;
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 4px;
		height: 40px;
		padding: 0 8px 0 14px;
		border-bottom: 1px solid var(--line);
	}

	header h1 {
		font-size: 13px;
	}

	.grow {
		flex: 1;
	}

	.btn.on {
		color: var(--ink);
		background: var(--pressed);
	}

	.reader {
		flex: 1;
		min-height: 0;
		padding: 16px 28px 28px;
		font-size: 13.5px;
		line-height: 1.6;
	}

	.reader:focus-visible {
		outline: none;
	}

	.reader > :global(*) {
		max-width: 72ch;
	}

	.reader :global(h1) {
		margin-bottom: 4px;
		font-size: 17px;
	}

	.reader :global(h2) {
		margin: 20px 0 6px;
		font-size: 13.5px;
	}

	.reader :global(p),
	.reader :global(ul),
	.reader :global(ol) {
		margin: 0 0 10px;
	}

	.reader :global(ul),
	.reader :global(ol) {
		padding-left: 1.4em;
	}

	.reader :global(ul) {
		list-style: square;
	}

	.reader :global(ol) {
		list-style: decimal;
	}

	.reader :global(li + li) {
		margin-top: 4px;
	}

	.reader :global(table) {
		display: block;
		max-width: 100%;
		margin: 6px 0 14px;
		overflow-x: auto;
		border-collapse: collapse;
		font-size: 12.5px;
	}

	.reader :global(th),
	.reader :global(td) {
		padding: 5px 14px 5px 0;
		border-bottom: 1px solid var(--line);
		text-align: left;
		vertical-align: top;
	}

	.reader :global(th) {
		font: 400 11px/1.4 var(--rb-label);
		text-transform: uppercase;
		color: var(--muted);
	}

	.reader :global(code) {
		padding: 0.08em 0.3em;
		border-radius: 3px;
		background: var(--board);
		font: 0.9em var(--rb-data);
	}
</style>
