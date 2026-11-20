<script lang="ts">
	import { getAccount } from '#account';
	import { app } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { agoShort, budgetOf, count, elapsed, gb, installedVariant, size, variantName } from '#lib/format.ts';
	import type { Package, Variant } from '#lib/types.ts';
	import { mod, nameOf, room, speedWord, versionLabel } from '#lib/words.ts';
	import Hint from './Hint.svelte';
	import Icon from './Icon.svelte';
	import Track from './Track.svelte';

	// The bay: one row along the bottom of the window. The machine's model memory budget is a
	// ruled track; what is in memory is a chamfered module seated in it, to scale. Around it:
	// who is using the app, how fast the answer is coming, downloads, and what waits to be sent.

	const flags = $derived(ux.flags);
	let now = $state(Date.now());
	let trackWidth = $state(0);
	let open = $state(false);
	let unloading = $state(false);
	let root = $state<HTMLElement>();

	const budget = $derived(app.system ? budgetOf(app.system) : null);

	interface Seat {
		pkg: Package;
		build: Variant;
		ratio: number;
		over: boolean;
	}

	function seat(packageId: string | undefined, variantId?: string): Seat | null {
		const pkg = app.packages.find((p) => p.id === packageId);
		if (!pkg || !budget) return null;
		const build = pkg.variants.find((v) => v.id === variantId) ?? installedVariant(pkg) ?? pkg.variants[0];
		if (!build) return null;
		const ratio = build.accel_mem_mb / budget.mb;
		return { pkg, build, ratio: Math.min(1, ratio), over: ratio > 1 };
	}

	const loaded = $derived(seat(chat.engine?.package, chat.engine?.variant));
	/** A model on its way into memory: nothing is seated yet, or another one is leaving. */
	const incoming = $derived(chat.work && chat.work.kind !== 'generating' ? seat(chat.work.package) : null);
	const ghost = $derived(app.preview && !(loaded && loaded.pkg.id === app.preview.package && loaded.build.id === app.preview.variant) ? seat(app.preview.package, app.preview.variant) : null);
	const generating = $derived(chat.work?.kind === 'generating');
	const working = $derived(chat.work ? app.packages.find((p) => p.id === chat.work!.package) : undefined);

	const downloads = $derived(
		Object.entries(app.installs)
			.filter(([, i]) => i.state === 'running')
			.map(([id, i]) => {
				const pkg = app.packages.find((p) => p.id === id);
				return { id, name: pkg ? nameOf(pkg, flags) : id, ratio: i.total > 0 ? Math.min(1, i.done / i.total) : 0 };
			})
	);
	const failedDownload = $derived(Object.values(app.installs).some((i) => i.state === 'failed'));

	// A clock for elapsed times; it only runs fast while something is being waited for.
	$effect(() => {
		const fast = chat.work !== null;
		now = Date.now();
		const timer = setInterval(() => (now = Date.now()), fast ? 500 : 20_000);
		return () => clearInterval(timer);
	});

	// The totals and the queue move when an answer or a picture ends.
	let wasBusy = false;
	$effect(() => {
		const busy = chat.busy;
		if (wasBusy && !busy) setTimeout(() => app.refreshUsage(), 600);
		wasBusy = busy;
	});

	// Whether a label fits inside its module; otherwise it sits on the free track next to it.
	const fits = (s: Seat, chars: number) => s.ratio * trackWidth >= chars * 6.7 + 18;

	async function unload() {
		app.track('unload_clicked', { from: 'bay' });
		unloading = true;
		try {
			await chat.unload();
			app.refreshSystem();
			open = false;
		} catch {
			chat.syncEngine();
		} finally {
			unloading = false;
		}
	}

	function onwindowclick(event: MouseEvent) {
		if (open && root && !root.contains(event.target as Node)) open = false;
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && open) {
			event.stopPropagation();
			open = false;
		}
	}

	const account = $derived(getAccount());
	const user = $derived(account.user);
	const initial = $derived(user && !user.anonymous ? (user.name ?? user.email ?? '').trim().charAt(0).toUpperCase() : '');
	/** A signed-in person by name; anyone else is simply "You". */
	const who = $derived(user && !user.anonymous ? account.label : 'You');
	const sync = $derived(app.usage?.sync ?? null);
	const syncText = $derived.by(() => {
		if (!sync) return '';
		if (sync.pending > 0) return flags.queue === 'full' ? `${count(sync.pending)} queued` : `${count(sync.pending)} to send`;
		if (sync.last_sent_ms) {
			const when = agoShort(sync.last_sent_ms, now);
			return when === 'now' ? 'sent just now' : `sent ${when} ago`;
		}
		return 'nothing to send';
	});
	const waited = $derived(chat.work ? (now - chat.work.since) / 1000 : 0);

	/** The model's name as this level says it, with its version where the level shows one. */
	const called = (s: Seat) => nameOf(s.pkg, flags);
	const version = (s: Seat) => versionLabel(s.pkg, s.build.id, flags);
	/** The memory label: Pro names where the budget is; the others just say memory. */
	const cap = $derived(budget ? (flags.techDetails ? `${budget.where} mem` : 'Memory') : '');

	/** The plain line (Guided): what is in memory and how much room is left. */
	const plainLine = $derived.by(() => {
		if (!budget) return '';
		if (incoming) return `${chat.work?.kind === 'switching' ? 'Moving' : 'Loading'} ${called(incoming)} into memory… ${elapsed(waited)}`;
		if (loaded) return `${called(loaded)} is loaded · ${room(loaded.build.accel_mem_mb, budget.mb)}`;
		return 'Nothing loaded yet · plenty of room';
	});
</script>

<svelte:window onclick={onwindowclick} />

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<footer class="bay" bind:this={root} {onkeydown} aria-label="Status">
	<button class="chip" class:current={app.screen === 'profile'} onclick={() => app.go('profile')} title="Your profile and activity" aria-label="Your profile: {who}" data-tour="profile">
		<span class="avatar" class:named={initial !== ''}>{#if initial}{initial}{:else}<Icon name="person" size={13} />{/if}</span>
		<span class="who">{who}</span>
	</button>

	{#if app.link === 'down'}
		<div class="down" role="status">
			<span class="dot"></span>
			<span class="label ink">Reconnecting to the local service</span>
			<span class="more">Downloads continue where they stopped.</span>
		</div>
	{:else if budget && !flags.ruler}
		<div class="mem plain" data-tour="memory">
			<span class="label cap" title="Memory models may use: {size(budget.mb)} of {size(budget.of)}">Memory</span>
			<button
				class="plainbar"
				onclick={() => (open = !open)}
				disabled={!loaded}
				aria-haspopup="dialog"
				aria-expanded={open}
				aria-label={loaded ? `${called(loaded)} uses ${size(loaded.build.accel_mem_mb)} of ${size(budget.mb)}. Open to unload.` : 'Nothing is loaded'}
				title={loaded ? `${called(loaded)} uses ${size(loaded.build.accel_mem_mb)} of the ${size(budget.mb)} models may use` : 'Nothing is in memory'}
			>
				{#if loaded}<span class="fill" class:generating style:width="{Math.max(4, loaded.ratio * 100)}%"></span>{:else if incoming}<span class="fill incoming" style:width="{Math.max(4, incoming.ratio * 100)}%"></span>{/if}
			</button>
			<span class="say" aria-live="polite">{plainLine}</span>
			<Hint topic="one-model" />
		</div>
	{:else if budget}
		<div class="mem" data-tour="memory">
			<span class="label cap" title="Memory models may use on the {budget.where}: {size(budget.mb)} of {size(budget.of)}">{cap}</span>
			<div class="rail" bind:clientWidth={trackWidth}>
				<Track budget={budget.mb} height={22} label="Model memory: {loaded ? `${loaded.pkg.name} takes ${size(loaded.build.accel_mem_mb)} of ${size(budget.mb)}` : `nothing loaded, ${size(budget.mb)} free`}">
					{#if loaded}
						{@const text = `${called(loaded)} ${version(loaded)} · ${size(loaded.build.accel_mem_mb)}`}
						{@const inside = fits(loaded, text.length)}
						<button
							class="module"
							class:generating
							class:open
							style:width="{loaded.ratio * 100}%"
							onclick={() => (open = !open)}
							aria-haspopup="dialog"
							aria-expanded={open}
							aria-label="{called(loaded)} {version(loaded)} is in memory, {size(loaded.build.accel_mem_mb)}. Open to unload."
						>
							{#if inside}<span class="text"><span class="name">{called(loaded)}</span><span class="num">{version(loaded) ? `${version(loaded)} · ` : ''}{size(loaded.build.accel_mem_mb)}</span></span>{/if}
							{#if generating}<span class="cursor"></span>{/if}
						</button>
						{#if !inside && !ghost}
							<button class="beside" style:left="{loaded.ratio * 100}%" onclick={() => (open = !open)} tabindex="-1" aria-hidden="true">
								<span class="name">{called(loaded)}</span><span class="num">{version(loaded) ? `${version(loaded)} · ` : ''}{size(loaded.build.accel_mem_mb)}</span>
							</button>
						{/if}
					{:else if incoming}
						<div class="module incoming" style:width="{incoming.ratio * 100}%"></div>
						<span class="beside" style:left="{incoming.ratio * 100}%">
							<span class="name">{chat.work?.kind === 'switching' ? (flags.techDetails ? 'Switching to' : 'Moving in') : 'Loading'} {called(incoming)}</span><span class="num">{size(incoming.build.accel_mem_mb)} · {elapsed(waited)}</span>
						</span>
					{:else if !ghost}
						<span class="vacant label">{flags.techDetails ? 'Nothing in memory' : 'Nothing loaded yet'}</span>
					{/if}

					{#if ghost}
						<div class="module ghost" class:over={ghost.over} style:width="{ghost.ratio * 100}%"></div>
						<span class="beside hint" class:over={ghost.over} style:left="{Math.max(ghost.ratio, loaded?.ratio ?? 0) * 100}%">
							{#if ghost.over}
								<span class="name">{called(ghost)} {version(ghost)}</span><span class="num">needs {size(ghost.build.accel_mem_mb)}, over by {size(ghost.build.accel_mem_mb - budget.mb)}</span>
							{:else}
								<span class="name">{called(ghost)} {version(ghost)}</span><span class="num">would take {size(ghost.build.accel_mem_mb)}</span>
							{/if}
						</span>
					{/if}
				</Track>
			</div>
			<span class="num free" title="Room left for models">{loaded ? gb(Math.max(0, budget.mb - loaded.build.accel_mem_mb)) : gb(budget.mb)} GB free</span>
			{#if flags.hints}<Hint topic="one-model" />{/if}
		</div>
	{:else}
		<div class="mem"></div>
	{/if}

	{#if flags.speedNumbers}
		<div class="cell rate" class:live={generating && chat.rate > 0} aria-live="off" title={chat.failure ? chat.failure : 'Speed of the answer, in tokens per second'}>
			{#if chat.work && chat.work.kind !== 'generating'}
				<span class="dot"></span><span class="num">{chat.work.kind} {elapsed(waited)}</span>
			{:else if generating && working?.modality === 'image'}
				<span class="dot"></span><span class="num">image {elapsed(waited)}</span>
			{:else if generating}
				<span class="dot"></span><span class="num">{chat.rate > 0 ? chat.rate.toFixed(1) : '…'} tok/s</span>
			{:else if chat.failure}
				<span class="num failed">last turn failed</span>
			{:else if chat.lastRate > 0}
				<span class="num idle">{chat.lastRate.toFixed(1)} tok/s</span>
			{:else}
				<span class="num idle">idle</span>
			{/if}
		</div>
	{:else if (chat.work && (flags.ruler || chat.work.kind === 'generating')) || chat.failure || (flags.speedWords && chat.lastRate > 0)}
		<!-- Guided: the memory line already says what is loading; this cell only says what is being made. -->
		<!-- Words, not figures: the number is on hover where the level shows speeds at all. -->
		{@const rate = generating && chat.rate > 0 ? chat.rate : chat.lastRate}
		<div class="cell rate" class:live={generating} title={flags.speedWords && rate > 0 ? `${rate.toFixed(1)} tokens per second. A token is a short word or part of one.` : ''}>
			{#if chat.work && chat.work.kind !== 'generating'}
				<span class="dot"></span><span>{chat.work.kind === 'switching' ? 'Switching' : 'Loading'} {elapsed(waited)}</span>
			{:else if generating && working?.modality === 'image'}
				<span class="dot"></span><span>Making a picture {elapsed(waited)}</span>
			{:else if generating}
				<span class="dot"></span><span>Writing{flags.speedWords && chat.rate > 0 ? ` · ${speedWord(chat.rate)}` : '…'}</span>
			{:else if chat.failure}
				<span class="failed">Last answer failed</span>
			{:else}
				<span class="idle">Last answer {speedWord(chat.lastRate)}</span>
			{/if}
		</div>
	{/if}

	{#each downloads as d (d.id)}
		<button class="cell pull" onclick={() => app.showModel(d.id)} title="Downloading {d.name}. Open Models.">
			<Icon name="download" size={13} />
			<span class="name">{d.name}</span>
			<span class="meter" aria-hidden="true"><span style:width="{d.ratio * 100}%"></span></span>
			<span class="num">{Math.floor(d.ratio * 100)}%</span>
		</button>
	{/each}
	{#if failedDownload && downloads.length === 0}
		<button class="cell pull" onclick={() => app.go('models')} title="Open Models to see why"><span class="failed" class:num={flags.techDetails}>{flags.techDetails ? 'download failed' : 'A download failed'}</span></button>
	{/if}

	{#if sync && flags.queue !== 'none'}
		<button class="cell sync" class:warn={Boolean(sync.last_error) && sync.pending > 0} onclick={() => app.go('profile')} title={sync.last_error && sync.pending > 0 ? `Not sent: ${sync.last_error}. Rigbrew keeps trying.` : 'Usage data waiting to be sent. What it is, in your profile.'}>
			<Icon name="sync" size={13} />
			<span class="num">{syncText}</span>
		</button>
	{/if}

	{#if open && loaded && budget}
		<div class="pop" role="dialog" aria-label="{loaded.pkg.name} in memory">
			<p class="title"><span class="name">{called(loaded)}</span> <span class="num">{version(loaded)}</span></p>
			<p class="num facts">
				{#if flags.techDetails}
					{size(loaded.build.accel_mem_mb)} of {size(budget.mb)} {budget.where} budget<br />
					{generating ? 'answering now' : 'in memory, idle'}
				{:else}
					Uses {size(loaded.build.accel_mem_mb)} of the {size(budget.mb)} models may use<br />
					{generating ? 'working now' : 'ready for the next message'}
				{/if}
			</p>
			<p class="explain">Rigbrew keeps one model in memory at a time. Unloading frees the memory now; the next message loads it again.</p>
			<div class="acts">
				<!-- svelte-ignore a11y_autofocus -->
				<button class="btn" onclick={unload} disabled={unloading || chat.busy} autofocus>
					<Icon name="eject" size={14} />
					{unloading ? 'Unloading…' : `Unload, free ${size(loaded.build.accel_mem_mb)}`}
					{#if flags.keyHints}<kbd>{mod}⇧U</kbd>{/if}
				</button>
				<button class="btn quiet" onclick={() => ((open = false), app.showModel(loaded.pkg.id))}>Show in Models</button>
			</div>
		</div>
	{/if}
</footer>

<style>
	.bay {
		position: relative;
		flex: none;
		display: flex;
		align-items: stretch;
		height: var(--bay);
		border-top: 1px solid var(--line);
		background: var(--board);
		font-size: 12px;
	}

	.name {
		font: 400 11px/16px var(--rb-label);
		letter-spacing: 0.02em;
		text-transform: uppercase;
		white-space: nowrap;
	}

	.num {
		font-size: 11.5px;
		white-space: nowrap;
	}

	/* Profile chip: bottom left, where the owner wants it. */
	.chip {
		flex: none;
		display: flex;
		align-items: center;
		gap: 7px;
		max-width: 190px;
		padding: 0 12px 0 10px;
		border-right: 1px solid var(--line);
		transition: background-color var(--quick);
	}

	.chip:hover,
	.chip.current {
		background: var(--hover);
	}

	.chip.current {
		box-shadow: inset 0 2px 0 var(--ink);
	}

	.avatar {
		flex: none;
		display: grid;
		place-items: center;
		width: 20px;
		height: 20px;
		border-radius: 3px;
		box-shadow: inset 0 0 0 1px var(--line);
		background: var(--bg);
		color: var(--muted);
		font: 600 11px/1 var(--rb-display);
	}

	.avatar.named {
		box-shadow: none;
		background: var(--ink);
		color: var(--bg);
	}

	.who {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-weight: 500;
	}

	/* The memory map */
	.mem {
		flex: 1;
		min-width: 0;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 0 12px;
	}

	.cap {
		flex: none;
	}

	/* Guided: a plain bar and a sentence instead of the ruler, only as wide as they need. */
	.mem.plain {
		flex: 0 1 auto;
		margin-right: auto;
	}

	.plainbar {
		flex: none;
		position: relative;
		width: clamp(70px, 14vw, 180px);
		height: 10px;
		border-radius: 2px;
		background: color-mix(in srgb, var(--line) 70%, var(--bg));
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--muted) 45%, var(--line));
		overflow: hidden;
	}

	.plainbar:not(:disabled):hover {
		box-shadow: inset 0 0 0 1px var(--muted);
	}

	.plainbar:disabled {
		opacity: 1;
	}

	.fill {
		--cut: 4px;
		position: absolute;
		left: 0;
		top: 0;
		bottom: 0;
		clip-path: var(--chamfer);
		background: var(--accent);
		transition: width 0.35s var(--ease);
	}

	.fill.incoming {
		background: repeating-linear-gradient(135deg, var(--accent) 0 3px, transparent 3px 7px);
	}

	.fill.generating {
		animation: blink 1.4s steps(2, jump-none) infinite;
	}

	.plain .say {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12.5px;
	}

	.rail {
		flex: 1;
		min-width: 0;
	}

	.free {
		flex: none;
		color: var(--muted);
	}

	/* A model seated in the track: the chamfered module of the mark. */
	.module {
		--cut: 7px;
		position: absolute;
		left: 0;
		bottom: 0;
		height: 19px;
		min-width: 9px;
		display: flex;
		align-items: center;
		padding: 0 8px 0 7px;
		clip-path: var(--chamfer);
		background: var(--accent);
		color: var(--on-accent);
		overflow: hidden;
		animation: seat 0.28s var(--ease) both;
		transition:
			width 0.35s var(--ease),
			background-color var(--quick);
	}

	@keyframes seat {
		from {
			transform: translateY(-6px);
			opacity: 0;
		}
	}

	button.module:hover,
	button.module.open {
		background: color-mix(in srgb, var(--accent) 84%, var(--ink));
	}

	button.module:focus-visible {
		outline: none;
		background: var(--ink);
		color: var(--bg);
	}

	.module .text {
		display: flex;
		align-items: baseline;
		gap: 7px;
		min-width: 0;
		overflow: hidden;
	}

	.module .num {
		opacity: 0.86;
	}

	/* Working: a cursor at the module's leading edge. */
	.cursor {
		position: absolute;
		right: 9px;
		bottom: 5px;
		width: 5px;
		height: 9px;
		background: currentColor;
		animation: blink 0.8s steps(2, jump-none) infinite;
	}

	/* On its way in: not seated yet. */
	.module.incoming {
		background: repeating-linear-gradient(135deg, var(--accent) 0 3px, transparent 3px 7px);
		background-size: 200% 100%;
		animation: slide 0.9s linear infinite;
	}

	@keyframes slide {
		to {
			background-position: -19.8px 0;
		}
	}

	/* Where a build would sit: an outline, nothing loaded. */
	.module.ghost {
		z-index: 1;
		clip-path: none;
		background: color-mix(in srgb, var(--fit) 14%, transparent);
		box-shadow: inset 0 0 0 1px var(--fit);
		animation: none;
		pointer-events: none;
	}

	.module.ghost.over {
		background: color-mix(in srgb, var(--fail) 14%, transparent);
		box-shadow: inset 0 0 0 1px var(--fail);
	}

	.beside {
		position: absolute;
		bottom: 0;
		height: 19px;
		display: flex;
		align-items: center;
		gap: 7px;
		padding: 0 8px;
		background: var(--board);
		color: var(--ink);
		white-space: nowrap;
		animation: appear 0.2s var(--ease) both;
	}

	.beside .num {
		color: var(--muted);
	}

	.beside.hint {
		z-index: 2;
		color: var(--fit);
	}

	.beside.hint.over {
		color: var(--fail);
	}

	.vacant {
		position: absolute;
		left: 8px;
		bottom: 5px;
		padding-right: 6px;
		background: var(--board);
		color: var(--muted);
	}

	/* Readouts */
	.cell {
		flex: none;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 12px;
		border-left: 1px solid var(--line);
		color: var(--muted);
		transition: background-color var(--quick);
	}

	button.cell:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.rate {
		min-width: 104px;
		justify-content: flex-end;
	}

	.rate.live {
		color: var(--ink);
	}

	.rate .idle {
		color: var(--muted);
	}

	.failed {
		color: var(--fail);
	}

	.dot {
		width: 6px;
		height: 6px;
		background: var(--accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}

	.pull .name {
		color: var(--ink);
	}

	.meter {
		width: 44px;
		height: 4px;
		background: var(--line);
	}

	.meter span {
		display: block;
		height: 100%;
		background: var(--accent);
		transition: width 0.3s var(--ease);
	}

	.sync.warn :global(svg) {
		color: var(--warn);
	}

	.down {
		flex: 1;
		display: flex;
		align-items: center;
		gap: 9px;
		padding: 0 12px;
		min-width: 0;
	}

	.down .dot {
		background: var(--warn);
	}

	.down .more {
		color: var(--muted);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	/* What a click on the module opens. */
	.pop {
		position: absolute;
		left: 12px;
		bottom: calc(100% + 6px);
		z-index: 40;
		width: 300px;
		margin-left: var(--chip-offset, 150px);
		padding: 10px 12px 12px;
		border: 1px solid var(--line);
		border-radius: var(--r);
		background: var(--bg);
		box-shadow: 0 8px 28px -8px var(--shade);
		display: flex;
		flex-direction: column;
		gap: 6px;
		animation: appear 0.14s var(--ease) both;
	}

	.pop .title {
		display: flex;
		align-items: baseline;
		gap: 7px;
	}

	.pop .title .name {
		font: 600 13px/1.3 var(--rb-display);
		letter-spacing: -0.045em;
		text-transform: none;
	}

	.pop .facts {
		color: var(--muted);
		line-height: 1.5;
	}

	.pop .explain {
		color: var(--muted);
		font-size: 12px;
	}

	.acts {
		display: flex;
		gap: 6px;
		margin-top: 4px;
	}

	.acts kbd {
		margin-left: 2px;
	}

	@media (max-width: 900px) {
		.who,
		.mem:not(.plain) .cap,
		.free,
		.pull .name,
		.down .more {
			display: none;
		}

		.chip {
			padding: 0 9px;
		}

		.pop {
			margin-left: 40px;
		}

		.rate {
			min-width: 0;
		}
	}
</style>
