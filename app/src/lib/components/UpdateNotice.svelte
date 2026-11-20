<script lang="ts">
	import { APP_VERSION, app } from '#lib/state.svelte.ts';
	import Icon from './Icon.svelte';

	// A newer version exists and this one still works: say so once, out of the way.
</script>

<aside class="notice" aria-label="Update available">
	<p class="head">
		<span class="label ink">Update available</span>
		<span class="num">{APP_VERSION} → {app.config?.latest_version ?? ''}</span>
		<button class="close" onclick={() => app.dismissUpdate()} aria-label="Dismiss" title="Dismiss"><Icon name="close" size={13} /></button>
	</p>
	{#if app.config?.notes}<p class="notes selectable">{app.config.notes}</p>{/if}
	<p class="actions">
		{#if app.config?.download_url}<button class="btn primary" onclick={() => app.acceptUpdate()}>Update</button>{/if}
		<button class="btn quiet" onclick={() => app.dismissUpdate()}>Later</button>
	</p>
</aside>

<style>
	.notice {
		position: absolute;
		top: 46px;
		right: 12px;
		z-index: 20;
		width: min(300px, calc(100% - 24px));
		display: flex;
		flex-direction: column;
		gap: 6px;
		padding: 8px 10px 10px;
		border: 1px solid var(--line);
		border-radius: var(--r);
		background: var(--bg);
		box-shadow: 0 10px 30px -12px var(--shade);
		animation: appear 0.18s var(--ease) both;
	}

	.head {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.head .num {
		font-size: 11.5px;
		color: var(--muted);
	}

	.close {
		display: grid;
		place-items: center;
		width: 20px;
		height: 20px;
		margin-left: auto;
		border-radius: 3px;
		color: var(--muted);
	}

	.close:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.notes {
		color: var(--muted);
		font-size: 12px;
	}

	.actions {
		display: flex;
		gap: 6px;
	}
</style>
