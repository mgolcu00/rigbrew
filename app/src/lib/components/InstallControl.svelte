<script lang="ts">
	import { app } from '#lib/state.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { plain } from '#lib/errors.ts';
	import { size } from '#lib/format.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import type { Package } from '#lib/types.ts';
	import Icon from './Icon.svelte';

	// The install action for one build of a package, on one line: the button with its download
	// size, the running download with a cancel, or what went wrong. The build is the
	// recommended one unless named.
	let {
		pkg,
		variant,
		primary = false,
		from = 'models',
		quiet = false
	}: {
		pkg: Package;
		variant?: string;
		primary?: boolean;
		from?: 'models' | 'chat' | 'first_run' | 'tip' | 'palette';
		/** The row says what went wrong itself; only the button stays here. */
		quiet?: boolean;
	} = $props();

	const rec = $derived(pkg.recommendation);
	const id = $derived(variant ?? ('variant' in rec ? rec.variant : ''));
	const build = $derived(pkg.variants.find((v) => v.id === id));
	const install = $derived(app.installs[pkg.id]);
	const mine = $derived(install && install.variant === id ? install : undefined);
	const ratio = $derived(mine && mine.total > 0 ? Math.min(1, mine.done / mine.total) : 0);
	const mb = (bytes: number) => bytes / 1024 / 1024;
	const free = $derived(app.system?.storage.free_mb ?? null);
	const noRoom = $derived(build && free !== null && build.disk_mb > free);

	let cancelling = $state(false);
	const failed = $derived(mine?.state === 'failed' ? plain(mine.error) : null);
	$effect(() => {
		if (failed) app.track('error_shown', { class: failed.id, where: 'install' });
	});

	async function cancel() {
		cancelling = true;
		try {
			await app.cancelInstall(pkg.id);
			app.say(`Stopped downloading ${pkg.name}. What is on disk stays; installing again continues from there.`);
		} catch (e) {
			app.say(`Could not stop the download. ${plain(e).text}`);
		} finally {
			cancelling = false;
		}
	}
</script>

{#if mine?.state === 'running'}
	<span class="progress" aria-live="polite">
		<span class="track" role="progressbar" aria-valuemin={0} aria-valuemax={100} aria-valuenow={Math.round(ratio * 100)} aria-label="Downloading {pkg.name}">
			<span class="bar" style:width="{ratio * 100}%"></span>
		</span>
		<span class="num" title="{size(mb(mine.done))} of {size(mb(mine.total))}">{Math.floor(ratio * 100)}% of {size(mb(mine.total))}</span>
		<button class="btn quiet" onclick={cancel} disabled={cancelling || !mine.job} title="Stop this download. What is downloaded stays, so installing again continues.">Cancel</button>
	</span>
{:else if build}
	<span class="row">
		{#if failed && !quiet}
			<span class="problem selectable" role="alert" title="{failed.text}{ux.flags.rawErrors || !failed.known ? `\n\n${failed.raw}` : ''}"><Icon name="warn" size={13} /> {ux.flags.rawErrors ? failed.raw : failed.short}</span>
			{#if failed.fix === 'help-disk'}<button class="btn quiet" onclick={() => guide.openHelp('disk')}>Free up space</button>{/if}
		{/if}
		{#if noRoom}
			<span class="muted num">needs {size(build.disk_mb)} of disk, {size(free ?? 0)} free</span>
		{:else}
			<button class="btn" class:primary onclick={() => (app.track('install_clicked', { package: pkg.id, variant: id, from, again: mine?.state === 'failed' }), app.install(pkg, id))} disabled={install?.state === 'running'} title="Downloads {size(build.disk_mb)}{free !== null ? `; ${size(free)} free on this disk` : ''}">
				<Icon name="download" size={14} />
				{mine?.state === 'failed' ? 'Try again' : 'Install'}
				<span class="num weight">{size(build.disk_mb)}</span>
			</button>
		{/if}
	</span>
{/if}

<style>
	.row,
	.progress {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}

	.weight {
		font-size: 11.5px;
		font-weight: 400;
		opacity: 0.8;
	}

	.progress {
		color: var(--muted);
		font-size: 11.5px;
		white-space: nowrap;
	}

	.track {
		width: 56px;
		height: 4px;
		background: var(--line);
	}

	.bar {
		display: block;
		height: 100%;
		background: var(--accent);
		transition: width 0.3s var(--ease);
	}

	.problem {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		max-width: 260px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12px;
	}
</style>
