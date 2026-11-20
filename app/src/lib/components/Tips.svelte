<script lang="ts">
	import { guide } from '#lib/guide.svelte.ts';

	// One tip at a time, the first time something happens: a line on top of the bay, where it
	// covers nothing (the level suggestion waits while it is there). It stays until put away.
	const tip = $derived(guide.tip);
</script>

{#if tip}
	<aside class="tip" aria-label="Tip" role="status">
		<span class="label ink">Tip</span>
		<p class="text">{tip.text}</p>
		<span class="acts">
			{#if tip.help}<button class="btn quiet" onclick={() => (guide.openHelp(tip.help, 'tip'), guide.dismissTip('more'))}>Tell me more</button>{/if}
			<button class="btn" onclick={() => guide.dismissTip('got_it')}>Got it</button>
		</span>
	</aside>
{/if}

<style>
	.tip {
		flex: none;
		display: flex;
		align-items: center;
		gap: 12px;
		min-height: 34px;
		padding: 4px 8px 4px 10px;
		border-top: 1px solid var(--line);
		box-shadow: inset 2px 0 0 var(--accent);
		background: var(--bg);
		animation: appear 0.18s var(--ease) both;
	}

	.text {
		flex: 1;
		min-width: 0;
		font-size: 12.5px;
		line-height: 1.45;
	}

	.acts {
		flex: none;
		display: flex;
		gap: 4px;
	}
</style>
