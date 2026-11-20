<script lang="ts">
	import { LEVELS, ux } from '#lib/experience.svelte.ts';
	import { guide, type ChecklistId } from '#lib/guide.svelte.ts';
	import { app } from '#lib/state.svelte.ts';
	import Icon from './Icon.svelte';

	// Getting started: a handful of first steps that tick themselves from what was really done
	// (the service's totals, visits and searches counted on this install). Each row does its
	// step on a click. Done, it folds into one line and offers the next level.
	let { act }: { act: (id: ChecklistId) => void } = $props();

	const items = $derived(guide.checklist(app.usage, app.packages.some((p) => p.modality === 'image')));
	const done = $derived(items.filter((i) => i.done).length);
	const all = $derived(done === items.length);
	const next = $derived(ux.next);

	// Each step is counted once, when it is first seen done.
	$effect(() => {
		guide.reportChecklist(
			items.filter((i) => i.done).map((i) => i.id),
			items.length
		);
	});

	function upgrade(to: NonNullable<typeof next>) {
		app.track('level_suggestion_accepted', { to, where: 'checklist' });
		app.setLevel(to, 'suggestion');
	}
</script>

{#if !guide.checklistHidden}
	{#if all}
		<div class="basics" role="status">
			<Icon name="check" size={15} />
			<p>
				<b>You know the basics.</b>
				{#if next}{LEVELS[next].name} also shows {next === 'standard' ? 'model versions, speeds and how memory is used' : 'every build and its measurements'}.{/if}
			</p>
			{#if next}<button class="btn" onclick={() => upgrade(next)}>Switch to {LEVELS[next].name}</button>{/if}
			<button class="btn quiet" onclick={() => guide.hideChecklist()}>Hide</button>
		</div>
	{:else}
		<section class="list" aria-label="Getting started">
			<button class="head" onclick={() => guide.setChecklistOpen(!guide.checklistOpen)} aria-expanded={guide.checklistOpen}>
				<span class="caret" class:open={guide.checklistOpen}><Icon name="chevron" size={12} /></span>
				<span class="title">Getting started</span>
				<span class="progress" aria-hidden="true">
					{#each items as item (item.id)}<span class:on={item.done}></span>{/each}
				</span>
				<span class="num count">{done} of {items.length} done</span>
			</button>
			{#if guide.checklistOpen}
				<ol>
					{#each items as item (item.id)}
						<li>
							<button class:done={item.done} onclick={() => act(item.id)}>
								<span class="box" aria-hidden="true">{#if item.done}<Icon name="check" size={12} />{/if}</span>
								<span class="say">{item.label}</span>
								<span class="sr">{item.done ? '(done)' : ''}</span>
							</button>
						</li>
					{/each}
				</ol>
			{/if}
		</section>
	{/if}
{/if}

<style>
	.list {
		border-top: 1px solid var(--line);
	}

	.head {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		height: 30px;
		border-radius: var(--r-s);
	}

	.head:hover .title {
		color: var(--accent);
	}

	.caret {
		display: grid;
		color: var(--muted);
		transition: transform var(--quick);
	}

	.caret.open {
		transform: rotate(90deg);
	}

	.title {
		font: 600 12.5px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.progress {
		display: inline-flex;
		gap: 2px;
		margin-left: 4px;
	}

	.progress span {
		width: 14px;
		height: 4px;
		background: var(--line);
	}

	.progress span.on {
		background: var(--accent);
	}

	.count {
		margin-left: auto;
		font-size: 11.5px;
		color: var(--muted);
	}

	ol {
		columns: 2 220px;
		column-gap: 24px;
		padding: 0 0 4px 20px;
	}

	li {
		break-inside: avoid;
	}

	li button {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		min-height: 26px;
		padding: 2px 6px 2px 0;
		border-radius: var(--r-s);
		text-align: left;
	}

	li button:hover .say {
		color: var(--accent);
	}

	.box {
		flex: none;
		display: grid;
		place-items: center;
		width: 14px;
		height: 14px;
		border-radius: 3px;
		box-shadow: inset 0 0 0 1px var(--muted);
		color: var(--bg);
	}

	.done .box {
		box-shadow: none;
		background: var(--ink);
	}

	.done .say {
		color: var(--muted);
	}

	.basics {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 6px 10px;
		padding: 8px 0;
		border-top: 1px solid var(--line);
		border-bottom: 1px solid var(--line-soft);
		font-size: 12.5px;
	}

	.basics p {
		flex: 1;
		min-width: 220px;
		color: var(--muted);
	}

	.basics b {
		font-weight: 600;
		color: var(--ink);
	}
</style>
