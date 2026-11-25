<script lang="ts">
	import { app } from '#lib/state.svelte.ts';
	import type { Package } from '#lib/types.ts';
	import InstallControl from './InstallControl.svelte';
	import StudioIcon from './StudioIcon.svelte';

	// What Studio cannot work without, and the way to get it, in the place where the work would
	// start: the video model makes the shots, the chat model writes them. Until both are here
	// nothing is written and nothing is made. The install control is the one Models uses: it
	// asks for an account and for the licence terms where a model has them.

	let {
		chat = true,
		band = false
	}: {
		/** Whether the chat model is asked for too: writing shots needs it, making them does not. */
		chat?: boolean;
		/** One line across the workstation instead of a card. */
		band?: boolean;
	} = $props();

	/** The package to install for a kind of work: the first one this machine can run, else the first there is. */
	const offered = (kind: Package['modality']) => {
		const all = app.packages.filter((p) => p.modality === kind);
		return all.find((p) => p.recommendation.verdict !== 'too_heavy') ?? all[0] ?? null;
	};

	const missing = $derived(
		[
			{ kind: 'video' as const, have: Boolean(app.videoPackage), title: 'Studio needs the video model', does: 'It makes the shots.' },
			{ kind: 'chat' as const, have: !chat || app.chatPackages.length > 0, title: 'Studio needs the chat model', does: 'It writes the shots.' }
		]
			.filter((m) => !m.have)
			.map((m) => ({ ...m, pkg: offered(m.kind) }))
	);
</script>

{#if missing.length > 0}
	<div class="needs" class:band role="group" aria-label="What Studio needs">
		{#each missing as m (m.kind)}
			<div class="need">
				<span class="mark"><StudioIcon name={m.kind === 'video' ? 'film' : 'spark'} size={band ? 12 : 15} /></span>
				<div class="text">
					<p class="title">{m.title}</p>
					{#if !band}<p class="does">{m.does}{m.pkg ? ` ${m.pkg.name}.` : ''}</p>{/if}
				</div>
				<span class="act">
					{#if !m.pkg}
						<span class="cannot">This build has none.</span>
					{:else if m.pkg.recommendation.verdict === 'too_heavy'}
						<span class="cannot">This computer cannot run it.</span>
					{:else}
						<InstallControl pkg={m.pkg} primary from="tip" />
					{/if}
				</span>
			</div>
		{/each}
	</div>
{/if}

<style>
	.needs {
		display: grid;
		border-radius: 5px;
		background: var(--st-raised);
		box-shadow:
			inset 0 1px 0 rgba(255, 255, 255, 0.05),
			0 0 0 1px #000;
		user-select: text;
		-webkit-user-select: text;
	}

	.need {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 10px 12px;
		padding: 12px;
	}

	.need + .need {
		border-top: 1px solid var(--st-edge);
	}

	.mark {
		flex: none;
		width: 30px;
		height: 30px;
		display: grid;
		place-items: center;
		border-radius: 4px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
		color: var(--st-muted);
	}

	.text {
		flex: 1 1 180px;
		min-width: 0;
		display: grid;
		gap: 3px;
	}

	.title {
		font: 600 13px/1.2 var(--rb-text);
		color: var(--st-ink);
	}

	.does {
		font-size: 11.5px;
		color: var(--st-muted);
	}

	.act {
		flex: 0 1 auto;
		min-width: 0;
		display: inline-flex;
		align-items: center;
		font-size: 12px;
	}

	.cannot {
		font-size: 11.5px;
		color: var(--st-warn);
	}

	/* One line under the control bar of an open film. */
	.band {
		flex: none;
		display: flex;
		border-radius: 0;
		background: #1d1a12;
		box-shadow: none;
		border-bottom: 1px solid var(--st-edge);
	}

	.band .need {
		flex: 1 1 0;
		gap: 8px;
		padding: 4px 10px;
		min-width: 0;
	}

	.band .need + .need {
		border-top: 0;
		border-left: 1px solid var(--st-edge);
	}

	.band .mark {
		width: 20px;
		height: 20px;
		background: none;
		box-shadow: none;
		color: var(--st-warn);
	}

	.band .text {
		flex: 0 1 auto;
	}

	.band .title {
		font-size: 11.5px;
		font-weight: 500;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
</style>
