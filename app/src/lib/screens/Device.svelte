<script lang="ts">
	import { onMount } from 'svelte';
	import { app } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { backendName, budgetOf, clock, familyOf, gb, osName, size, tierOf, variantName } from '#lib/format.ts';
	import type { Package } from '#lib/types.ts';
	import { machine, machineLine, nameOf, roleOf, room, versionLabel } from '#lib/words.ts';
	import Hint from '#lib/components/Hint.svelte';
	import Track from '#lib/components/Track.svelte';

	// Device: the machine as a spec sheet. What the scan read, the memory budget drawn with
	// the same ruled track as the bay, and every family seated in it: what fits, what does
	// not, and why.

	const system = $derived(app.system!);
	const tier = $derived(tierOf(system));
	const budget = $derived(budgetOf(system));
	const gpu = $derived(system.gpus[0]);
	let readAt = $state(Date.now());
	let reading = $state(false);

	async function read() {
		reading = true;
		await app.refreshSystem();
		readAt = Date.now();
		reading = false;
	}

	onMount(() => {
		read();
	});

	const cores = $derived(system.cpu.physical_cores === system.cpu.logical_cores ? `${system.cpu.physical_cores} cores` : `${system.cpu.physical_cores} cores, ${system.cpu.logical_cores} threads`);

	const flags = $derived(ux.flags);
	const home = $derived(system.storage.data_dir.replace(/^\/Users\/[^/]+/, '~'));

	const specs = $derived.by((): { name: string; value: string; note: string }[] => {
		if (!flags.techDetails) {
			// Plain: what each part means for running models, without instruction sets or architectures.
			const plainRows = [
				{ name: 'Processor', value: system.cpu.brand, note: cores },
				{ name: 'Memory', value: size(system.ram_total_mb), note: `${size(system.ram_available_mb)} free at ${clock(readAt)}` }
			];
			if (system.gpus.length === 0) plainRows.push({ name: 'Graphics', value: 'None that Rigbrew can use', note: 'models run on the processor' });
			for (const g of system.gpus) {
				plainRows.push({ name: 'Graphics', value: g.name, note: g.kind === 'unified' ? 'shares the memory above' : g.memory_total_mb ? `${size(g.memory_total_mb)} of its own memory` : '' });
			}
			plainRows.push({ name: 'Disk', value: `${size(system.storage.free_mb)} free`, note: `models are kept in ${home}` });
			plainRows.push({ name: 'System', value: `${osName(system.os.family)} ${system.os.version}`, note: '' });
			return plainRows;
		}
		const rows = [
			{ name: 'Class', value: tier.label, note: `hardware class ${system.tier}` },
			{ name: 'Processor', value: system.cpu.brand, note: [cores, ...system.cpu.features.map((f) => f.toUpperCase())].join(' · ') },
			{ name: 'Memory', value: size(system.ram_total_mb), note: `${size(system.ram_available_mb)} free at ${clock(readAt)}` }
		];
		if (system.gpus.length === 0) rows.push({ name: 'Graphics', value: 'No supported GPU', note: 'builds run on the processor' });
		for (const g of system.gpus) {
			const memory = g.memory_total_mb ? `${size(g.memory_total_mb)} ${g.kind === 'unified' ? 'shared with the system' : 'of its own'}` : '';
			rows.push({ name: 'Graphics', value: g.name, note: [backendName(g.backend), memory].filter(Boolean).join(' · ') });
		}
		rows.push({ name: 'Disk', value: `${size(system.storage.free_mb)} free`, note: system.storage.data_dir });
		rows.push({ name: 'System', value: `${osName(system.os.family)} ${system.os.version}`, note: system.os.arch });
		return rows;
	});

	interface Seat {
		pkg: Package;
		build: string;
		mb: number;
		budget: number;
		where: string;
		fits: boolean;
		line: string;
	}

	const seats = $derived(
		app.packages.map((pkg): Seat => {
			const rec = pkg.recommendation;
			if (rec.verdict === 'run') {
				const onGpu = rec.placement === 'accelerator';
				const max = onGpu ? system.accel_budget_mb : system.cpu_budget_mb;
				const others = pkg.variants.filter((v) => v.accel_mem_mb > max).length;
				return {
					pkg,
					build: rec.variant,
					mb: rec.memory_mb,
					budget: max,
					where: onGpu ? 'GPU' : 'CPU',
					fits: true,
					line: flags.techDetails
						? `${pkg.variants.length === 1 ? 'its one build fits' : others === 0 ? `all ${pkg.variants.length} builds fit` : `${pkg.variants.length - others} of ${pkg.variants.length} builds fit`}${rec.compromise ? '; the pick is below the quality bar' : ''}`
						: flags.plainNames
							? `Fits, with ${room(rec.memory_mb, max)} left.`
							: `${pkg.variants.length === 1 ? 'Fits' : others === 0 ? `Every version fits` : `${pkg.variants.length - others} of ${pkg.variants.length} versions fit`}; Rigbrew recommends ${pkg.variants.length === 1 ? 'it' : `the one that uses ${size(rec.memory_mb)}`}.`
				};
			}
			if (rec.verdict === 'needs_disk') {
				const v = pkg.variants.find((x) => x.id === rec.variant);
				return { pkg, build: rec.variant, mb: v?.accel_mem_mb ?? 0, budget: budget.mb, where: budget.where, fits: false, line: `Fits in memory, but needs ${size(rec.needed_mb)} of disk and ${size(rec.free_mb)} is free.` };
			}
			return { pkg, build: '', mb: rec.smallest_mb, budget: rec.budget_mb, where: budget.where, fits: false, line: flags.techDetails ? `the smallest build needs ${size(rec.smallest_mb)}; the budget is ${size(rec.budget_mb)}` : `Even its smallest version needs ${size(rec.smallest_mb)}; models may use ${size(rec.budget_mb)} here.` };
		})
	);

	const loaded = $derived(chat.engine?.package ?? '');
	const reserve = $derived(Math.max(0, budget.of - budget.mb));
</script>

<section class="screen" aria-label="Device">
	<header class="screen-head">
		<h1>{flags.techDetails ? 'Device' : `Your ${machine}`}</h1>
		<p class="num sub selectable">{system.cpu.brand}{flags.techDetails ? ` · ${tier.label}` : ''}</p>
		<span class="grow"></span>
		{#if flags.techDetails}<span class="num sub">read {clock(readAt)}</span>{/if}
		<button class="btn" onclick={read} disabled={reading} title="Read memory and disk again">{reading ? 'Reading…' : 'Read again'}</button>
	</header>

	<div class="body scroll">
		<div class="grid">
			<section class="specs" aria-label="What the scan read">
				<h2 class="rule label">Read from this {flags.techDetails ? 'machine' : machine}</h2>
				<dl class="selectable">
					{#each specs as spec, i (spec.name + i)}
						<div>
							<dt class="label">{spec.name}</dt>
							<dd>
								<span class="value">{spec.value}</span>
								{#if spec.note}<span class="note" class:num={flags.techDetails}>{spec.note}</span>{/if}
							</dd>
						</div>
					{/each}
				</dl>
				<p class="line">{flags.techDetails ? tier.line : machineLine(system)}</p>
			</section>

			<section class="budget" aria-label="The memory budget">
				<h2 class="rule label">What models may use</h2>
				<p class="figure">
					<strong class="num">{size(budget.mb)}</strong>
					{#if flags.techDetails}
						<span>of {size(budget.of)} {gpu?.kind === 'unified' ? 'unified memory' : budget.where === 'GPU' ? 'graphics memory' : 'memory'}, on the {budget.where}</span>
					{:else}
						<span>of {size(budget.of)} {budget.where === 'GPU' && gpu?.kind !== 'unified' ? 'graphics memory' : 'memory'}</span>
						<Hint topic="fits" />
					{/if}
				</p>
				<div class="whole" style:--budget="{(budget.mb / budget.of) * 100}%">
					<div class="in"><Track budget={budget.mb} height={34} numbers={flags.ruler} label="Models may use {size(budget.mb)}" /></div>
					<div class="out" title="Kept for the system and other programs"><span class="label">kept free</span><span class="num">{gb(reserve)} GB</span></div>
				</div>
				<p class="explain">
					{#if flags.techDetails}
						Rigbrew never hands a model all of the memory. {size(reserve)} stays with the system and your other programs; a build is recommended only when it fits inside the
						{size(budget.mb)} that is left. One model is in memory at a time, so each family below is measured against the whole budget.
					{:else}
						Rigbrew never gives a model all of the memory: {size(reserve)} stays with the system and your other apps. A model is recommended only when it fits in the {size(budget.mb)} that is left.
						Only one model is in memory at a time, so each one below is measured against all of it.
					{/if}
				</p>
				{#if budget.where === 'GPU' && flags.techDetails}
					<p class="explain second"><span class="num">{size(system.cpu_budget_mb)}</span> is the budget for builds that run on the processor instead (voice, listening).</p>
				{/if}

				<h2 class="rule label fits">{flags.techDetails ? 'What fits, and what does not' : 'Each model on this ' + machine}</h2>
				<ul class="seats">
					{#each seats as seat (seat.pkg.id)}
						{@const family = familyOf(seat.pkg)}
						<li class:no={!seat.fits}>
							<button class="who" onclick={() => app.showModel(seat.pkg.id)} title="Show {seat.pkg.name} in Models">
								<span class="pname">{nameOf(seat.pkg, flags)}</span>
								{#if seat.build && (flags.buildIds || flags.versionNames)}<span class:num={flags.buildIds}>{versionLabel(seat.pkg, seat.build, flags)}</span>{/if}
								{#if !flags.plainNames}<span class="tag">{flags.techDetails ? family.does : roleOf(seat.pkg)}</span>{/if}
							</button>
							<div class="rail">
								<Track budget={seat.budget} height={16} numbers={false} label="{seat.pkg.name}: {size(seat.mb)} of {size(seat.budget)}">
									<span class="module" class:over={!seat.fits && seat.mb > seat.budget} class:live={loaded === seat.pkg.id} style:width="{Math.min(1, seat.mb / Math.max(seat.budget, 1)) * 100}%"></span>
								</Track>
							</div>
							<span class="num take">{#if flags.techDetails}{gb(seat.mb)}<span class="of">&nbsp;/ {gb(seat.budget)} GB {seat.where}</span>{:else}uses {size(seat.mb)}{/if}</span>
							<span class="verdict">
								{#if seat.fits}<span class="tag fit">fits</span>{:else}<span class="tag warn">does not fit</span>{/if}
								{#if seat.pkg.installed.length > 0}<span class="tag accent">installed</span>{/if}
								{#if loaded === seat.pkg.id}<span class="tag accent">{flags.techDetails ? 'in memory' : flags.plainNames ? 'ready' : 'loaded'}</span>{/if}
							</span>
							<span class="why">{seat.line}</span>
						</li>
					{/each}
				</ul>
			</section>
		</div>
	</div>
</section>

<style>
	.sub {
		font-size: 12px;
		color: var(--muted);
		white-space: nowrap;
	}

	.grow {
		flex: 1;
	}

	.body {
		flex: 1;
		min-height: 0;
		padding: 8px var(--gutter) 28px;
	}

	.grid {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		gap: 12px 40px;
		max-width: 1500px;
	}

	@media (min-width: 1000px) {
		.grid {
			grid-template-columns: minmax(320px, 5fr) minmax(0, 8fr);
		}
	}

	.specs dl div {
		display: grid;
		grid-template-columns: 92px minmax(0, 1fr);
		gap: 12px;
		padding: 7px 0 8px;
		border-bottom: 1px solid var(--line-soft);
	}

	.specs dt {
		padding-top: 2px;
	}

	.specs dd {
		display: flex;
		flex-direction: column;
		min-width: 0;
	}

	.value {
		font: 600 13px/1.35 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.note {
		font-size: 11.5px;
		color: var(--muted);
		overflow-wrap: anywhere;
	}

	.line {
		margin-top: 10px;
		color: var(--muted);
		max-width: 52ch;
	}

	.figure {
		display: flex;
		align-items: baseline;
		gap: 10px;
		margin: 4px 0 10px;
		color: var(--muted);
	}

	.figure strong {
		font-size: 26px;
		font-weight: 500;
		letter-spacing: -0.03em;
		line-height: 1;
		color: var(--ink);
	}

	/* All of the memory: the budget as a ruled track, and what is kept free beside it. */
	.whole {
		display: grid;
		grid-template-columns: var(--budget) minmax(0, 1fr);
		align-items: end;
	}

	.out {
		height: 34px;
		display: flex;
		flex-direction: column;
		justify-content: flex-end;
		gap: 0;
		padding: 0 0 3px 8px;
		border-bottom: 1px solid var(--line);
		background: repeating-linear-gradient(135deg, var(--line-soft) 0 1px, transparent 1px 6px);
		overflow: hidden;
	}

	.out .num {
		font-size: 11px;
		color: var(--muted);
	}

	.explain {
		margin-top: 10px;
		max-width: 76ch;
		color: var(--muted);
	}

	.explain.second {
		margin-top: 4px;
	}

	.explain .num {
		color: var(--ink);
		font-size: 12px;
	}

	.fits {
		margin-top: 20px;
	}

	.seats li {
		display: grid;
		grid-template-columns: minmax(150px, 210px) minmax(80px, 1fr) 132px;
		grid-template-areas: 'who rail take' 'verdict why why';
		align-items: center;
		gap: 2px 14px;
		padding: 8px 0 9px;
		border-bottom: 1px solid var(--line-soft);
	}

	.who {
		grid-area: who;
		display: flex;
		align-items: center;
		gap: 7px;
		min-width: 0;
		white-space: nowrap;
	}

	.who:hover .pname {
		color: var(--accent);
	}

	.pname {
		font: 600 12.5px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.who > span:not(.pname):not(.tag) {
		font-size: 12px;
		color: var(--muted);
	}

	.rail {
		grid-area: rail;
		min-width: 0;
	}

	.module {
		--cut: 5px;
		position: absolute;
		left: 0;
		bottom: 0;
		height: 12px;
		min-width: 6px;
		clip-path: var(--chamfer);
		background: var(--fit);
	}

	.module.live {
		background: var(--accent);
	}

	.no .module {
		background: var(--faint);
	}

	.module.over {
		clip-path: none;
		background: repeating-linear-gradient(135deg, var(--fail) 0 2px, transparent 2px 5px);
	}

	.take {
		grid-area: take;
		font-size: 12px;
		text-align: right;
		white-space: nowrap;
	}

	.of {
		color: var(--faint);
	}

	.verdict {
		grid-area: verdict;
		display: flex;
		gap: 4px;
	}

	.why {
		grid-area: why;
		font-size: 12px;
		color: var(--muted);
	}

	@media (max-width: 860px) {
		.seats li {
			grid-template-columns: minmax(0, 1fr) 120px;
			grid-template-areas: 'who take' 'rail rail' 'verdict verdict' 'why why';
		}
	}
</style>
