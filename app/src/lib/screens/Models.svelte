<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { app } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { machine } from '#lib/words.ts';
	import Hint from '#lib/components/Hint.svelte';
	import ModelList from '#lib/components/ModelList.svelte';
	import { RUNS_LATER, budgetOf, bytes, families, familyOf, gb, installedVariant, measures, percent, sentence, size, variantName, type Family } from '#lib/format.ts';
	import type { Package, Variant } from '#lib/types.ts';
	import Icon from '#lib/components/Icon.svelte';
	import InstallControl from '#lib/components/InstallControl.svelte';
	import RemoveControl from '#lib/components/RemoveControl.svelte';
	import Track from '#lib/components/Track.svelte';

	// Models: one table. Families are the groups, builds are the rows. Every row carries what
	// an instrument would read off it: size on disk, memory against this machine's budget,
	// measured quality and speed, where it stands, and what can be done with it.

	// Disk figures move while this screen is open (downloads, removals).
	onMount(() => {
		app.refreshSystem();
		app.refreshUsage();
		return () => (app.preview = null);
	});

	const flags = $derived(ux.flags);

	// The first time something here does not fit this machine, say what that means.
	$effect(() => {
		if (app.packages.some((p) => p.recommendation.verdict === 'too_heavy')) app.tip('nofit');
	});

	type SortKey = 'catalogue' | 'disk' | 'memory' | 'quality' | 'speed';
	let sort = $state<{ key: SortKey; down: boolean }>({ key: 'catalogue', down: false });
	let onlyFamily = $state('');
	let onlyFits = $state(false);
	let onlyInstalled = $state(false);
	let opened = $state<Record<string, boolean>>({});

	// The Speed column and the second state tag are the first to go in a narrow window.
	let innerWidth = $state(1200);
	const roomy = $derived(innerWidth >= 1040);
	const span = $derived(roomy ? 7 : 6);

	const system = $derived(app.system);
	const budgetHere = $derived(system ? budgetOf(system) : null);
	const installedCount = $derived(app.packages.reduce((n, p) => n + p.installed.length, 0));
	// What the installed builds take: measured by the service when it says, else the catalogue's figures.
	const taken = $derived(app.usage?.disk.total_bytes ?? app.packages.reduce((sum, p) => sum + p.installed.reduce((s, v) => s + app.diskOf(p, v), 0), 0));
	const free = $derived(app.usage ? app.usage.disk.free_bytes / 1024 / 1024 : (system?.storage.free_mb ?? null));

	/** The budget a package's builds are placed in: the GPU's, or the processor's when that is where it runs. */
	function budgetFor(pkg: Package): { mb: number; where: string } {
		const rec = pkg.recommendation;
		if (!system) return { mb: 0, where: '' };
		if (rec.verdict === 'run') return rec.placement === 'accelerator' ? { mb: system.accel_budget_mb, where: 'GPU' } : { mb: system.cpu_budget_mb, where: 'CPU' };
		if (rec.verdict === 'too_heavy') return { mb: rec.budget_mb, where: budgetHere?.where ?? '' };
		return budgetHere ?? { mb: 0, where: '' };
	}

	/** A speed figure for sorting and for the Speed column, whatever the family measures. */
	function speed(v: Variant): { value: number; text: string } | null {
		for (const [key, raw] of Object.entries(v.metrics)) {
			if (typeof raw !== 'number') continue;
			if (key.endsWith('_tok_s')) return { value: raw, text: `${raw.toFixed(1)} tok/s` };
			if (key === 'rtf_1_thread') return { value: raw, text: `${raw.toFixed(2)}× real time` };
		}
		return null;
	}

	const qualityKnown = (v: Variant) => v.metrics.quality_measured !== false;

	interface Row {
		pkg: Package;
		build: Variant;
		key: string;
		budget: { mb: number; where: string };
		fits: boolean;
		installed: boolean;
		recommended: boolean;
	}

	const shelves = $derived.by(() => {
		const known: Family[] = [...families, ...app.packages.map(familyOf).filter((f) => !families.some((k) => k.id === f.id))].filter(
			(family, i, all) => all.findIndex((f) => f.id === family.id) === i
		);
		return known
			.map((family) => {
				const packages = app.packages.filter((p) => familyOf(p).id === family.id);
				let rows: Row[] = packages.flatMap((pkg) => {
					const budget = budgetFor(pkg);
					const rec = pkg.recommendation;
					return pkg.variants.map((build) => ({
						pkg,
						build,
						key: `${pkg.id}/${build.id}`,
						budget,
						fits: build.accel_mem_mb <= budget.mb,
						installed: pkg.installed.includes(build.id),
						recommended: 'variant' in rec && rec.variant === build.id
					}));
				});
				const total = rows.length;
				if (onlyFits) rows = rows.filter((r) => r.fits);
				if (onlyInstalled) rows = rows.filter((r) => r.installed);
				const by: Record<SortKey, (r: Row) => number> = {
					catalogue: (r) => r.build.accel_mem_mb,
					disk: (r) => r.build.disk_mb,
					memory: (r) => r.build.accel_mem_mb,
					quality: (r) => (qualityKnown(r.build) ? r.build.quality : -1),
					speed: (r) => speed(r.build)?.value ?? -1
				};
				rows.sort((a, b) => (by[sort.key](a) - by[sort.key](b)) * (sort.down ? -1 : 1));
				return { family, packages, rows, total };
			})
			.filter((shelf) => shelf.packages.length > 0 && (!onlyFamily || shelf.family.id === onlyFamily));
	});

	const present = $derived(families.filter((f) => app.packages.some((p) => familyOf(p).id === f.id)));
	const shown = $derived(shelves.reduce((n, s) => n + s.rows.length, 0));

	function sortBy(key: SortKey) {
		sort = sort.key === key ? { key, down: !sort.down } : { key, down: key === 'quality' || key === 'speed' };
	}

	const arrow = (key: SortKey) => (sort.key === key ? (sort.down ? '↓' : '↑') : '');

	/** Makes this package the one the chat uses, and says so. */
	function use(pkg: Package) {
		if (pkg.modality === 'chat') {
			chat.model = pkg.id;
			chat.setMode('chat', 'click');
			app.say(`${pkg.name} answers the next message.`);
		} else {
			chat.setMode('image', 'click');
			app.say(`Image mode is on: the next message goes to ${pkg.name}.`);
		}
	}

	/** What the service loads for a package: the best build on disk. */
	const active = (row: Row) => row.installed && installedVariant(row.pkg)?.id === row.build.id;
	const inUse = (row: Row) => active(row) && ((row.pkg.modality === 'chat' && chat.model === row.pkg.id) || (row.pkg.modality === 'image' && chat.mode === 'image'));
	const inMemory = (row: Row) => chat.engine?.package === row.pkg.id && chat.engine.variant === row.build.id;

	function toggle(key: string) {
		opened[key] = !opened[key];
	}

	// The palette, the bay and the chat send people here for one package.
	$effect(() => {
		const id = app.spotlight;
		if (!id || !flags.buildsTable) return;
		const pkg = app.packages.find((p) => p.id === id);
		app.spotlight = '';
		if (!pkg) return;
		const rec = pkg.recommendation;
		const build = installedVariant(pkg)?.id ?? ('variant' in rec ? rec.variant : pkg.variants[0]?.id);
		onlyFamily = '';
		onlyFits = false;
		onlyInstalled = false;
		opened[`${pkg.id}/${build}`] = true;
		tick().then(() => document.getElementById(`row-${pkg.id}-${build}`)?.scrollIntoView({ block: 'center' }));
	});
</script>

<svelte:window bind:innerWidth />

<section class="screen" aria-label="Models">
	<header class="screen-head">
		<h1>Models</h1>
		{#if flags.buildsTable}
			<p class="num storage">
				{#if installedCount > 0}<b>{bytes(taken)}</b> in {installedCount} installed {installedCount === 1 ? 'build' : 'builds'}{:else}nothing installed{/if}{#if free !== null}<span class="sep"><b>{size(free)}</b> free on disk</span>{/if}{#if budgetHere}<span class="sep"><b>{size(budgetHere.mb)}</b> {budgetHere.where} budget</span>{/if}
			</p>
		{:else}
			{@const count = app.packages.filter((p) => p.installed.length > 0).length}
			<p class="storage plain">
				{#if count > 0}{count} installed, using <b class="num">{bytes(taken)}</b> of disk{:else}Nothing installed yet{/if}{#if free !== null}<span class="sep"><b class="num">{size(free)}</b> free</span>{/if}{#if budgetHere}<span class="sep">models may use <b class="num">{size(budgetHere.mb)}</b> of memory</span>{/if}
			</p>
			<Hint topic="models" />
		{/if}
		<span class="grow"></span>
		{#if flags.buildsTable}
		<div class="filters" role="group" aria-label="Filter">
			<button class:on={onlyFamily === ''} onclick={() => (onlyFamily = '')}>All</button>
			{#each present as family (family.id)}
				<button class:on={onlyFamily === family.id} onclick={() => (onlyFamily = onlyFamily === family.id ? '' : family.id)}>{family.does}</button>
			{/each}
		</div>
		<div class="filters" role="group" aria-label="Show only">
			<button class:on={onlyFits} aria-pressed={onlyFits} onclick={() => (onlyFits = !onlyFits)}><span class="pip fit"></span>Fits</button>
			<button class:on={onlyInstalled} aria-pressed={onlyInstalled} onclick={() => (onlyInstalled = !onlyInstalled)}>Installed</button>
		</div>
		{/if}
	</header>

	<div class="body scroll">
		{#if app.sponsors}
			<p class="support">
				{app.sponsors.headline || 'Rigbrew is free and open source.'}
				<button class="link" onclick={() => app.openSponsor('library')}>Support Rigbrew</button>
			</p>
		{/if}

		{#if !flags.buildsTable}
			<p class="intro">
				{#if flags.plainNames}Each model does one thing. Install what you want to use; Rigbrew picks the version that suits this {machine}.{:else}One row per model, with the version Rigbrew recommends for this {machine}. The other versions are under each one.{/if}
			</p>
			<ModelList />
		{:else}
		<table>
			<colgroup>
				<col class="c-name" />
				<col class="c-num" />
				<col class="c-fit" />
				<col class="c-num" />
				{#if roomy}<col class="c-speed" />{/if}
				<col class="c-state" />
				<col class="c-act" />
			</colgroup>
			<thead>
				<tr>
					<th scope="col"><button onclick={() => sortBy('catalogue')}>Build</button></th>
					<th scope="col" class="r" aria-sort={sort.key === 'disk' ? (sort.down ? 'descending' : 'ascending') : undefined}><button onclick={() => sortBy('disk')}>Disk {arrow('disk')}</button></th>
					<th scope="col" aria-sort={sort.key === 'memory' ? (sort.down ? 'descending' : 'ascending') : undefined}><button onclick={() => sortBy('memory')}>Memory in the budget {arrow('memory')}</button></th>
					<th scope="col" class="r" aria-sort={sort.key === 'quality' ? (sort.down ? 'descending' : 'ascending') : undefined}><button onclick={() => sortBy('quality')} title="Share of the full-precision model's behaviour this build keeps">Quality {arrow('quality')}</button></th>
					{#if roomy}<th scope="col" class="r" aria-sort={sort.key === 'speed' ? (sort.down ? 'descending' : 'ascending') : undefined}><button onclick={() => sortBy('speed')}>Speed {arrow('speed')}</button></th>{/if}
					<th scope="col">State</th>
					<th scope="col"><span class="sr">Actions</span></th>
				</tr>
			</thead>

			{#each shelves as shelf (shelf.family.id)}
				<tbody>
					<tr class="family">
						<th colspan={span} scope="colgroup">
							<span class="fam">
								<Icon name={shelf.family.icon} size={15} />
								<span class="fname">{shelf.family.name}</span>
								<span class="tag">{shelf.family.does}</span>
								<span class="can">{shelf.family.can}.{#if !shelf.family.runs}&nbsp;{RUNS_LATER}{/if}</span>
								<span class="num count">{shelf.rows.length === shelf.total ? `${shelf.total} ${shelf.total === 1 ? 'build' : 'builds'}` : `${shelf.rows.length} of ${shelf.total}`}</span>
							</span>
						</th>
					</tr>
					{#each shelf.rows as row (row.key)}
						{@const open = opened[row.key]}
						{@const pace = speed(row.build)}
						<!-- svelte-ignore a11y_no_noninteractive_element_interactions, a11y_click_events_have_key_events -->
						<tr
							class="build"
							class:open
							class:installed={row.installed}
							class:unfit={!row.fits}
							id="row-{row.pkg.id}-{row.build.id}"
							onmouseenter={() => (app.preview = { package: row.pkg.id, variant: row.build.id })}
							onmouseleave={() => (app.preview = null)}
							onclick={(e) => {
								if (!(e.target as HTMLElement).closest('button, a')) toggle(row.key);
							}}
						>
							<th scope="row">
								<button class="name" onclick={() => toggle(row.key)} aria-expanded={Boolean(open)} title={open ? 'Hide details' : 'Show details'}>
									<span class="caret" class:open><Icon name="chevron" size={12} /></span>
									<span class="pname">{row.pkg.name}</span>
									<span class="num vid">{variantName(row.build.id)}</span>
								</button>
							</th>
							<td class="num r">{row.installed ? bytes(app.diskOf(row.pkg, row.build.id)) : size(row.build.disk_mb)}</td>
							<td>
								<div class="fit" title="{size(row.build.accel_mem_mb)} of the {size(row.budget.mb)} {row.budget.where} budget">
									<div class="bar">
										<Track budget={row.budget.mb} height={11} numbers={false} label="{size(row.build.accel_mem_mb)} of {size(row.budget.mb)}">
											<span class="seat" class:over={!row.fits} style:width="{Math.min(1, row.build.accel_mem_mb / Math.max(row.budget.mb, 1)) * 100}%"></span>
										</Track>
									</div>
									<span class="num">{gb(row.build.accel_mem_mb)}<span class="of">&nbsp;/ {gb(row.budget.mb)} GB</span></span>
								</div>
							</td>
							<td class="num r">{#if qualityKnown(row.build)}{percent(row.build.quality)}{:else}<span class="none" title="Not measured yet">n/m</span>{/if}</td>
							{#if roomy}<td class="num r">{#if pace}{pace.text}{:else}<span class="none">–</span>{/if}</td>{/if}
							<td>
								<span class="states">
									{#if row.installed}
										<span class="tag accent">{inMemory(row) ? 'in memory' : 'installed'}</span>
										{#if inUse(row) && roomy}<span class="tag">in use</span>{/if}
									{:else if !row.fits}
										<span class="why">needs {size(row.build.accel_mem_mb)}, budget {size(row.budget.mb)}</span>
									{:else if row.recommended}
										<span class="tag fit">recommended</span>
									{:else}
										<span class="tag fit">fits</span>
									{/if}
								</span>
							</td>
							<td class="act">
								{#if row.installed}
									{#if shelf.family.runs && active(row) && !inUse(row)}
										<button class="btn" onclick={() => use(row.pkg)}>Use</button>
									{/if}
									{#if !app.installs[row.pkg.id]}<RemoveControl pkg={row.pkg} variant={row.build.id} />{/if}
								{:else if row.fits && row.pkg.recommendation.verdict !== 'needs_disk'}
									<InstallControl pkg={row.pkg} variant={row.build.id} primary={row.recommended && row.pkg.installed.length === 0 && shelf.family.runs} />
								{:else if row.pkg.recommendation.verdict === 'needs_disk'}
									<span class="why">needs {size(row.pkg.recommendation.needed_mb)} of disk</span>
								{/if}
							</td>
						</tr>
						{#if open}
							{@const measured = measures(row.build.metrics)}
							{@const rec = row.pkg.recommendation}
							<tr class="detail">
								<td colspan={span}>
									<div class="sheet selectable">
										<div>
											<h3 class="label">Measured</h3>
											{#if measured.list.length > 0}
												<dl>
													{#each measured.list as m (m.label)}
														<div><dd class="num">{m.value}</dd><dt>{m.label}</dt></div>
													{/each}
												</dl>
											{:else}
												<p class="muted">No measurements for this build yet.</p>
											{/if}
											{#if measured.notes.length > 0}<p class="muted note">{measured.notes.join(' ')}</p>{/if}
										</div>
										<div>
											<h3 class="label">On this machine</h3>
											<p>
												{#if !row.fits}
													Not for this machine: it needs {size(row.build.accel_mem_mb)} and the {row.budget.where} budget is {size(row.budget.mb)}.
												{:else if row.recommended && rec.verdict === 'run'}
													Recommended. {sentence(rec.reason)}{rec.compromise ? ' It is below the quality bar; nothing better fits.' : ''}
												{:else}
													Fits: {size(row.build.accel_mem_mb)} of the {size(row.budget.mb)} {row.budget.where} budget.
													{#if 'variant' in rec}The recommended build is {variantName(rec.variant)}.{/if}
												{/if}
											</p>
											{#if row.installed && !active(row)}
												<p class="muted note">{variantName(installedVariant(row.pkg)?.id ?? '')} is installed too and is the one Rigbrew loads: the best build on disk. This one can be removed.</p>
											{/if}
											{#if !shelf.family.runs}<p class="muted note">{RUNS_LATER}</p>{/if}
										</div>
										<div>
											<h3 class="label">Licence</h3>
											<p>{row.pkg.source.license}</p>
											<p class="muted note">Open-weight model. Optimized and measured by Rigbrew.</p>
											<h3 class="label second">Files</h3>
											<p class="num">{size(row.build.disk_mb)} on disk · {size(row.build.accel_mem_mb)} in memory</p>
										</div>
									</div>
								</td>
							</tr>
						{/if}
					{:else}
						<tr class="vacant"><td colspan={span}>No {shelf.family.does.toLowerCase()} build matches the filter.</td></tr>
					{/each}
				</tbody>
			{/each}
		</table>

		<p class="legend num">
			{shown} {shown === 1 ? 'build' : 'builds'} shown · rows open in place · pointing at a row shows where it would sit in the bay below
		</p>
		{/if}
	</div>
</section>

<style>
	.storage {
		font-size: 12px;
		color: var(--muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.storage b {
		font-weight: 500;
		color: var(--ink);
	}

	.sep::before {
		content: '·';
		margin: 0 7px;
		color: var(--faint);
	}

	.grow {
		flex: 1;
	}

	.filters {
		flex: none;
		display: inline-flex;
		gap: 1px;
		padding: 1px;
		border-radius: 4px;
		box-shadow: inset 0 0 0 1px var(--line);
	}

	.filters button {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		height: 20px;
		padding: 0 7px;
		border-radius: 3px;
		font: 400 11px/1 var(--rb-label);
		letter-spacing: 0.02em;
		text-transform: uppercase;
		color: var(--muted);
	}

	.filters button:hover {
		color: var(--ink);
	}

	.filters button.on {
		background: var(--ink);
		color: var(--bg);
	}

	.pip {
		width: 6px;
		height: 6px;
		background: var(--fit);
	}

	.on .pip {
		background: currentColor;
	}

	.body {
		flex: 1;
		min-height: 0;
		padding: 0 var(--gutter) 24px;
	}

	.support {
		padding: 7px 0;
		border-bottom: 1px solid var(--line-soft);
		font-size: 12px;
		color: var(--muted);
	}

	table {
		width: 100%;
		border-collapse: collapse;
		table-layout: fixed;
	}

	.c-name {
		width: 19%;
	}

	.c-num {
		width: 8%;
	}

	.c-fit {
		width: 22%;
	}

	.c-speed {
		width: 12%;
	}

	.c-state {
		width: 13%;
	}

	thead th {
		position: sticky;
		top: 0;
		z-index: 2;
		height: 28px;
		padding: 0 12px 0 0;
		background: var(--bg);
		box-shadow: inset 0 -1px 0 var(--line);
		text-align: left;
		font: 400 11px/1 var(--rb-label);
		letter-spacing: 0.02em;
		text-transform: uppercase;
		color: var(--muted);
		white-space: nowrap;
	}

	thead th button {
		font: inherit;
		letter-spacing: inherit;
		text-transform: inherit;
		color: inherit;
	}

	thead th button:hover,
	thead th[aria-sort] {
		color: var(--ink);
	}

	.r {
		text-align: right;
	}

	thead th.r {
		text-align: right;
	}

	/* A family: the group line. */
	.family th {
		height: 34px;
		padding: 10px 0 0;
		border-bottom: 1px solid var(--line);
		font-weight: 400;
		text-align: left;
		vertical-align: middle;
	}

	.fam {
		display: flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
	}

	.fname {
		font: 600 13px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.can {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12.5px;
		color: var(--muted);
	}

	.count {
		flex: none;
		font-size: 11.5px;
		color: var(--faint);
	}

	/* A build: one row. */
	.build > th,
	.build > td {
		height: 30px;
		padding: 0 12px 0 0;
		border-bottom: 1px solid var(--line-soft);
		font-weight: 400;
		text-align: left;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.build > td.r {
		text-align: right;
	}

	.build:hover > th,
	.build:hover > td,
	.build.open > th,
	.build.open > td {
		background: var(--well);
	}

	.build .num {
		font-size: 12px;
	}

	.name {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 100%;
		height: 100%;
		min-width: 0;
	}

	.caret {
		display: grid;
		color: var(--faint);
		transition: transform var(--quick);
	}

	.caret.open {
		transform: rotate(90deg);
		color: var(--ink);
	}

	.pname {
		font: 500 12.5px/1 var(--rb-display);
		letter-spacing: -0.045em;
		color: var(--muted);
	}

	.installed .pname {
		color: var(--ink);
		font-weight: 600;
	}

	.vid {
		color: var(--ink);
	}

	.unfit .pname,
	.unfit .vid,
	.unfit > td.r {
		color: var(--faint);
	}

	.fit {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.bar {
		flex: 1;
		min-width: 40px;
		max-width: 260px;
	}

	.fit .num {
		flex: none;
		min-width: 76px;
	}

	.of {
		color: var(--faint);
	}

	/* What a build takes, seated in the budget. Green: it fits. */
	.seat {
		--cut: 4px;
		position: absolute;
		left: 0;
		bottom: 0;
		height: 8px;
		min-width: 5px;
		clip-path: var(--chamfer);
		background: var(--fit);
	}

	.seat.over {
		clip-path: none;
		background: repeating-linear-gradient(135deg, var(--fail) 0 2px, transparent 2px 5px);
	}

	.none {
		color: var(--faint);
	}

	.states {
		display: inline-flex;
		align-items: center;
		gap: 4px;
	}

	.why {
		font-size: 12px;
		color: var(--muted);
	}

	td.act {
		padding-right: 0;
		text-align: right;
		overflow: visible;
	}

	.act :global(.btn) {
		vertical-align: middle;
	}

	.vacant td {
		height: 30px;
		border-bottom: 1px solid var(--line-soft);
		color: var(--faint);
		font-size: 12.5px;
	}

	/* Details, in place. */
	.detail td {
		padding: 0;
		border-bottom: 1px solid var(--line);
		background: var(--well);
	}

	.sheet {
		display: grid;
		grid-template-columns: minmax(0, 1.3fr) minmax(0, 1.2fr) minmax(0, 1fr);
		gap: 20px;
		padding: 8px 12px 14px 18px;
		font-size: 12.5px;
		animation: appear 0.14s var(--ease) both;
	}

	.sheet h3 {
		margin-bottom: 4px;
		font-weight: 400;
	}

	.sheet h3.second {
		margin-top: 10px;
	}

	.sheet dl div {
		display: grid;
		grid-template-columns: 96px 1fr;
		gap: 10px;
		padding: 2px 0;
	}

	.sheet dd {
		font-size: 12px;
	}

	.sheet dt {
		color: var(--muted);
	}

	.note {
		margin-top: 4px;
		font-size: 12px;
	}

	.storage.plain {
		font-size: 12.5px;
	}

	.intro {
		padding: 10px 0 10px;
		max-width: 72ch;
		color: var(--muted);
	}

	.legend {
		margin-top: 12px;
		font-size: 11px;
		color: var(--faint);
	}

	@media (max-width: 1040px) {
		.storage .sep:last-child {
			display: none;
		}

		.c-name {
			width: 24%;
		}

		.c-num {
			width: 10%;
		}

		.c-state {
			width: 17%;
		}
	}

	@media (max-width: 860px) {
		.storage {
			display: none;
		}

		.fit .num,
		.can {
			display: none;
		}

		.sheet {
			grid-template-columns: 1fr;
			gap: 10px;
		}
	}
</style>
