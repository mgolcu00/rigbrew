<script lang="ts">
	import { ux } from '#lib/experience.svelte.ts';
	import { codeOf, plain } from '#lib/errors.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { app } from '#lib/state.svelte.ts';
	import { onMount } from 'svelte';
	import Icon from './Icon.svelte';

	// An error as one calm line: what to do in plain words, a button when there is something to
	// do, and a short code. The service's own message is under "Details" in Pro only.
	let {
		error,
		lead = '',
		onretry,
		compact = false,
		where = 'chat'
	}: {
		error: unknown;
		/** What was being done, as a short sentence before the explanation ("No answer."). */
		lead?: string;
		onretry?: () => void;
		/** One line, for tight places (a table cell). */
		compact?: boolean;
		/** Where it is shown, for counting: chat, picture, open_chat. */
		where?: string;
	} = $props();

	const said = $derived(plain(error));
	// Counted by class only, never with the message.
	onMount(() => app.track('error_shown', { class: plain(error).id, where }));

	const action = $derived.by((): (() => void) | null => {
		switch (said.fix) {
			case 'retry':
				return onretry ?? null;
			case 'models':
				return () => app.go('models');
			case 'help-disk':
				// In Models the Remove buttons are right there; elsewhere, go to them.
				return () => (app.screen === 'models' ? guide.openHelp('disk', 'error') : app.go('models'));
			case 'help-fits':
				return () => guide.openHelp('fits', 'error');
			case 'privacy':
				return () => app.openPrivacy();
			default:
				return null;
		}
	});
	const details = $derived(said.raw && ux.flags.rawErrors);
	const code = $derived(codeOf(error));
</script>

<div class="error" class:compact role="alert">
	<p class="say selectable">
		<Icon name="warn" size={14} />
		<span>{#if lead}<b>{lead}</b>{' '}{/if}{said.text} <span class="code num" title="Error code">{code}</span></span>
	</p>
	{#if action || details}
		<div class="more">
			{#if action}<button class="btn" type="button" onclick={action}>{said.act}</button>{/if}
			{#if details}
				<details>
					<summary>Details</summary>
					<p class="raw num selectable">{said.raw}</p>
				</details>
			{/if}
		</div>
	{/if}
</div>

<style>
	.error {
		display: flex;
		flex-direction: column;
		gap: 6px;
		margin-top: 4px;
		font-size: 12.5px;
	}

	.say {
		display: flex;
		align-items: flex-start;
		gap: 6px;
		color: var(--muted);
	}

	.say :global(svg) {
		margin-top: 2px;
		color: var(--warn);
	}

	.code {
		margin-left: 4px;
		font-size: 11px;
		color: var(--faint);
	}

	.say b {
		font-weight: 600;
	}

	.more {
		display: flex;
		align-items: flex-start;
		flex-wrap: wrap;
		gap: 8px;
		padding-left: 20px;
	}

	details {
		min-width: 0;
		flex: 1;
		color: var(--muted);
	}

	summary {
		display: inline-flex;
		align-items: center;
		height: 24px;
		padding: 0 4px;
		border-radius: var(--r-s);
		font-size: 12px;
		cursor: default;
		list-style: none;
	}

	summary::-webkit-details-marker {
		display: none;
	}

	summary::before {
		content: '▸';
		margin-right: 4px;
		font-size: 10px;
	}

	details[open] summary::before {
		content: '▾';
	}

	summary:hover {
		color: var(--ink);
	}

	.raw {
		margin-top: 2px;
		padding: 4px 8px;
		border-left: 2px solid var(--line);
		font-size: 11.5px;
		overflow-wrap: anywhere;
	}

	.compact {
		flex-direction: row;
		align-items: center;
		flex-wrap: wrap;
		gap: 4px 8px;
		margin: 0;
		font-size: 12px;
	}

	.compact .say {
		align-items: center;
		min-width: 0;
	}

	.compact .say :global(svg) {
		margin: 0;
	}

	.compact .more {
		padding: 0;
	}
</style>
