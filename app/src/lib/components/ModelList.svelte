<script lang="ts">
	import { app } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { familyOf, installedVariant, size } from '#lib/format.ts';
	import type { Package, Variant } from '#lib/types.ts';
	import { machine, nameOf, plainDoes, roleOf, speedAnywhere, speedHere, speedWord, usesMemory, verdictOf, versionName, versionNames } from '#lib/words.ts';
	import ErrorLine from './ErrorLine.svelte';
	import Hint from './Hint.svelte';
	import Icon from './Icon.svelte';
	import InstallControl from './InstallControl.svelte';
	import RemoveControl from './RemoveControl.svelte';

	// Models for Guided and Standard: one row per model with one clear action. Guided says
	// what each one does and whether it suits this machine, in words, and installs the version
	// Rigbrew recommends. Standard adds which version that is, how fast it is, and the other
	// versions behind "Other versions". Pro has the full table instead (Models.svelte).

	const flags = $derived(ux.flags);
	const system = $derived(app.system);
	let opened = $state<Record<string, boolean>>({});

	/** The version a row talks about: the installed one, else the recommended one. */
	function main(pkg: Package): Variant | undefined {
		const rec = pkg.recommendation;
		return installedVariant(pkg) ?? pkg.variants.find((v) => 'variant' in rec && v.id === rec.variant) ?? [...pkg.variants].sort((a, b) => a.accel_mem_mb - b.accel_mem_mb)[0];
	}

	const budgetOf = (pkg: Package) => {
		const rec = pkg.recommendation;
		if (!system) return 0;
		if (rec.verdict === 'run') return rec.placement === 'accelerator' ? system.accel_budget_mb : system.cpu_budget_mb;
		if (rec.verdict === 'too_heavy') return rec.budget_mb;
		return system.accel_budget_mb || system.cpu_budget_mb;
	};

	function speedText(v: Variant): { text: string; title: string } | null {
		const here = speedHere(v, system);
		const any = here ?? speedAnywhere(v);
		if (any === null) return null;
		return { text: speedWord(any), title: `${any.toFixed(1)} tokens per second${here === null ? ', measured on another machine' : ' on this machine'}. A token is a short word or part of one.` };
	}

	/** What the row's button does for an installed model: go and use it. */
	function use(pkg: Package) {
		if (pkg.modality === 'chat') {
			chat.model = pkg.id;
			chat.newChat();
			chat.setMode('chat', 'click');
		} else {
			chat.newChat();
			chat.setMode(pkg.modality === 'video' ? 'video' : pkg.modality === 'image' ? 'image' : 'chat', 'click');
		}
		app.go('chat');
	}

	// The palette and the chat send people here for one model: open its other versions.
	$effect(() => {
		const id = app.spotlight;
		if (!id) return;
		app.spotlight = '';
		if (flags.otherVersions) opened[id] = true;
		queueMicrotask(() => document.getElementById(`model-${id}`)?.scrollIntoView({ block: 'center' }));
	});

	const rows = $derived(app.packages);
</script>

<ul class="models">
	{#each rows as pkg (pkg.id)}
		{@const family = familyOf(pkg)}
		{@const rec = pkg.recommendation}
		{@const v = main(pkg)}
		{@const installed = pkg.installed.length > 0}
		{@const verdict = verdictOf(pkg, system)}
		{@const speed = v && flags.speedWords ? speedText(v) : null}
		{@const others = pkg.variants.filter((x) => x.id !== v?.id).sort((a, b) => a.accel_mem_mb - b.accel_mem_mb)}
		{@const loaded = chat.engine?.package === pkg.id}
		<li id="model-{pkg.id}" class:installed class:unfit={!installed && !verdict.fits}>
			<span class="ico"><Icon name={family.icon} size={16} /></span>
			<div class="what">
				<p class="name">
					<span class="pname">{nameOf(pkg, flags)}</span>
					{#if flags.versionNames && v}<span class="version">{versionName(pkg, v.id)}</span>{/if}
					{#if flags.versionNames && !installed && 'variant' in rec && v?.id === rec.variant}<span class="tag fit" title="The version Rigbrew measured as the best fit for this machine">recommended</span>{/if}
					{#if !flags.plainNames}<span class="tag">{roleOf(pkg)}</span>{/if}
					{#if installed}<span class="tag accent" title={loaded ? 'In memory and ready to answer at once' : ''}>{loaded ? (flags.plainNames ? 'ready' : 'loaded') : 'installed'}</span>{/if}
				</p>
				<p class="does">{plainDoes(pkg)}{#if !family.runs}&nbsp;<b>Download only for now:</b> running it in Rigbrew comes later.{/if}</p>
				<p class="facts">
					{#if v}<span>{usesMemory(v.accel_mem_mb)}</span>{/if}
					{#if !family.runs && verdict.fits}
						<span class="sep fit">Fits this {machine}</span>
					{:else if installed || verdict.fits}
						<!-- Standard says the speed on its own; Guided folds it into the verdict. -->
						<span class="sep" class:fit={verdict.fits}>{flags.speedWords && verdict.fits ? `Fits this ${machine}` : verdict.text}</span>
					{:else}
						<span class="sep no">{verdict.text}</span>
					{/if}
					{#if speed}<span class="sep" title={speed.title}>{speed.text}</span>{/if}
					{#if v && !installed}<span class="sep">{size(v.disk_mb)} download</span>{/if}
					{#if !verdict.fits && !installed}<Hint topic="fits" />{/if}
				</p>
				{#if app.installs[pkg.id]?.state === 'failed'}
					<ErrorLine error={app.installs[pkg.id].error} lead="The download did not finish." where="install" />
				{/if}
				{#if flags.otherVersions && others.length > 0}
					<button class="more" onclick={() => (opened[pkg.id] = !opened[pkg.id])} aria-expanded={Boolean(opened[pkg.id])}>
						<span class="caret" class:open={opened[pkg.id]}><Icon name="chevron" size={11} /></span>
						Other versions ({others.length})
					</button>
					{#if opened[pkg.id]}
						{@const names = versionNames(pkg)}
						<table class="versions">
							<caption class="sr">Other versions of {pkg.name}</caption>
							<tbody>
								{#each others as o (o.id)}
									{@const fits = o.accel_mem_mb <= budgetOf(pkg)}
									{@const has = pkg.installed.includes(o.id)}
									{@const os = speedText(o)}
									<tr class:unfit={!fits}>
										<th scope="row">{names.get(o.id)}{#if 'variant' in rec && rec.variant === o.id}&nbsp;<span class="tag fit">recommended</span>{/if}</th>
										<td class="num">{size(o.accel_mem_mb)} memory</td>
										<td class="num">{size(o.disk_mb)} on disk</td>
										<td title={os?.title ?? ''}>{os?.text ?? ''}</td>
										<td class="act">
											{#if has}
												<span class="tag accent">installed</span>
												{#if !app.installs[pkg.id]}<RemoveControl {pkg} variant={o.id} />{/if}
											{:else if !fits}
												<span class="no">too big for this {machine}</span>
											{:else if rec.verdict !== 'needs_disk'}
												<InstallControl {pkg} variant={o.id} />
											{/if}
										</td>
									</tr>
								{/each}
							</tbody>
						</table>
						<p class="note">
							Smaller versions use less memory and disk; larger ones stay closer to the original. When two are installed, Rigbrew uses the better one.
							<Hint topic="versions" />
						</p>
					{/if}
				{/if}
			</div>
			<div class="act main">
				{#if installed}
					{#if family.runs}<button class="btn" onclick={() => use(pkg)}><Icon name={pkg.modality === 'image' ? 'image' : pkg.modality === 'video' ? 'video' : 'chat'} size={14} /> {pkg.modality === 'image' ? 'Make a picture' : pkg.modality === 'video' ? 'Make a clip' : 'Open a chat'}</button>{/if}
					{#if !app.installs[pkg.id]}<RemoveControl {pkg} variant={flags.otherVersions ? installedVariant(pkg)?.id : undefined} />{/if}
				{:else if rec.verdict === 'run'}
					<InstallControl {pkg} primary={family.runs && (pkg.modality === 'chat' || app.chatPackages.length > 0)} quiet />
				{/if}
			</div>
		</li>
	{/each}
</ul>

<style>
	.models {
		max-width: 1080px;
		border-top: 1px solid var(--line);
	}

	.models > li {
		display: grid;
		grid-template-columns: 32px minmax(0, 1fr) auto;
		align-items: start;
		gap: 4px 12px;
		padding: 12px 0;
		border-bottom: 1px solid var(--line-soft);
	}

	.ico {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		border-radius: var(--r);
		background: var(--well);
		box-shadow: inset 0 0 0 1px var(--line-soft);
	}

	.what {
		min-width: 0;
	}

	.name {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 4px 8px;
		min-height: 20px;
	}

	.pname {
		font: 600 14px/1.2 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.version {
		color: var(--muted);
		font-size: 12.5px;
	}

	.version::before {
		content: '·';
		margin-right: 6px;
		color: var(--faint);
	}

	.does {
		margin-top: 2px;
		color: var(--ink);
	}

	.does b {
		font-weight: 600;
	}

	.facts {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 2px 0;
		margin-top: 3px;
		font-size: 12.5px;
		color: var(--muted);
	}

	.sep::before {
		content: '·';
		margin: 0 7px;
		color: var(--faint);
	}

	.facts .fit {
		color: var(--fit);
	}

	.facts :global(.hint) {
		margin-left: 6px;
	}

	.no {
		color: var(--fail);
	}

	.unfit .pname,
	.unfit .does {
		color: var(--muted);
	}

	.act {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		flex-wrap: wrap;
		gap: 6px;
	}

	.act.main {
		padding-top: 2px;
	}

	.more {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		height: 24px;
		margin-top: 4px;
		margin-left: -2px;
		padding: 0 6px 0 2px;
		border-radius: var(--r-s);
		font-size: 12.5px;
		color: var(--accent);
	}

	.more:hover {
		background: var(--hover);
	}

	.caret {
		display: grid;
		transition: transform var(--quick);
	}

	.caret.open {
		transform: rotate(90deg);
	}

	.versions {
		width: 100%;
		margin-top: 4px;
		border-collapse: collapse;
		font-size: 12.5px;
	}

	.versions th,
	.versions td {
		height: 30px;
		padding: 0 12px 0 0;
		border-top: 1px solid var(--line-soft);
		text-align: left;
		font-weight: 400;
		white-space: nowrap;
	}

	.versions th {
		font-weight: 500;
	}

	.versions .num {
		color: var(--muted);
	}

	.versions td.act {
		padding-right: 0;
		display: table-cell;
		text-align: right;
	}

	.versions .unfit th,
	.versions .unfit td {
		color: var(--faint);
	}

	.note {
		margin-top: 6px;
		font-size: 12px;
		color: var(--muted);
	}

	@media (max-width: 760px) {
		.models > li {
			grid-template-columns: 32px minmax(0, 1fr);
		}

		.act.main {
			grid-column: 2;
			justify-content: flex-start;
		}

		.versions td:nth-child(3),
		.versions td:nth-child(4) {
			display: none;
		}
	}
</style>
