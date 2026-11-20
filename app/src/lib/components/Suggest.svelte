<script lang="ts">
	import { LEVELS } from '#lib/experience.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { app } from '#lib/state.svelte.ts';

	// The next level, suggested in one quiet line when real use says the person is ready.
	// "Show more" switches, "Not now" keeps it away for a week. It never switches by itself.
	const level = $derived(guide.suggestion(app.usage, app.packages));
	const shown = $derived(level !== null && guide.tour === null && guide.tip === null);

	$effect(() => {
		if (shown && level) guide.suggestedOnce(level);
	});

	function accept(to: typeof level) {
		if (!to) return;
		app.track('level_suggestion_accepted', { to });
		app.setLevel(to, 'suggestion');
	}
</script>

{#if shown && level}
	<div class="suggest" role="status">
		<p>{guide.reason(level, app.usage, app.packages)}</p>
		<span class="acts">
			<button class="btn" onclick={() => accept(level)} title="Switch to {LEVELS[level].name}">Show more</button>
			<button class="btn quiet" onclick={() => guide.snooze(level)}>Not now</button>
		</span>
	</div>
{/if}

<style>
	.suggest {
		flex: none;
		display: flex;
		align-items: center;
		gap: 12px;
		min-height: 34px;
		padding: 4px 8px 4px 12px;
		border-top: 1px solid var(--line);
		background: var(--accent-soft);
		font-size: 12.5px;
		animation: appear 0.18s var(--ease) both;
	}

	p {
		flex: 1;
		min-width: 0;
	}


	.acts {
		flex: none;
		display: flex;
		gap: 4px;
	}

</style>
