<script lang="ts">
	import { app } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { plain } from '#lib/errors.ts';
	import { bytes } from '#lib/format.ts';
	import { nameOf, versionLabel } from '#lib/words.ts';
	import type { Package } from '#lib/types.ts';
	import Icon from './Icon.svelte';

	// Removes one installed build, or every build of a package, after saying what that frees.
	let { pkg, variant, label = 'Remove' }: { pkg: Package; variant?: string; label?: string } = $props();

	let asking = $state(false);
	let busy = $state(false);
	let problem = $state('');

	const targets = $derived(variant ? [variant] : pkg.installed);
	const total = $derived(targets.reduce((sum, id) => sum + app.diskOf(pkg, id), 0));
	// Builds of one package can share files (a text encoder, a VAE). A shared file stays while
	// another build needs it, and is counted once, so only a lone build has an exact figure.
	const exact = $derived(pkg.installed.length === 1);
	const what = $derived([nameOf(pkg, ux.flags), variant ? versionLabel(pkg, variant, ux.flags) : ''].filter(Boolean).join(' · '));

	async function remove() {
		app.track('remove_clicked', { package: pkg.id, variant: variant ?? 'all' });
		busy = true;
		problem = '';
		try {
			const freed = await app.remove(pkg, variant);
			app.say(`Removed ${what}. Freed ${bytes(freed)}.`);
			asking = false;
		} catch (e) {
			problem = plain(e).text;
		} finally {
			busy = false;
		}
	}
</script>

{#if asking}
	<span class="ask" role="group" aria-label="Remove {what}">
		<span class="frees">Frees {exact ? '' : 'up to '}<b class="num">{bytes(total)}</b> of disk</span>
		<button class="btn danger" onclick={remove} disabled={busy}>{busy ? 'Removing…' : 'Remove'}</button>
		<button class="btn quiet" onclick={() => (asking = false)} disabled={busy}>Keep</button>
	</span>
{:else}
	<button class="btn quiet" onclick={() => (asking = true)} disabled={chat.busy} title="Remove {what} from this disk">
		<Icon name="trash" size={14} />
		{label}
	</button>
{/if}
{#if problem}<span class="problem selectable" role="alert">Could not remove it. {problem}</span>{/if}

<style>
	.ask {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		white-space: nowrap;
	}

	.frees {
		color: var(--muted);
		font-size: 12px;
	}

	.frees b {
		font-weight: 500;
		color: var(--ink);
	}

	.problem {
		font-size: 12px;
	}
</style>
