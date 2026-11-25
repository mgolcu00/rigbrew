<script lang="ts">
	import { onMount } from 'svelte';
	import { ACCOUNTS, AccountWelcome } from '#account';
	import { app } from '#lib/state.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { FAMILIES_EXPLAINED, RUNS_LATER, budgetOf, familyOf, rigFacts, size, tierOf, variantName } from '#lib/format.ts';
	import { machine, machineLine, nameOf, plainDoes } from '#lib/words.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import InstallControl from '#lib/components/InstallControl.svelte';
	import LegalSheet from '#lib/components/LegalSheet.svelte';
	import Mark from '#lib/components/Mark.svelte';
	import Track from '#lib/components/Track.svelte';

	// First launch: the terms, then the machine is read like an instrument reads a sample
	// (one line at a time, then the budget as a ruled track), then what to install.
	// Development only: `?welcome=scan` or `?welcome=models` opens the flow at that step, as a
	// first launch would see it (nothing is accepted or deleted by looking).
	const peek = import.meta.env.DEV ? new URLSearchParams(location.search).get('welcome') : null;
	let step = $state<Step>(peek === 'scan' ? 'scan' : peek === 'models' ? 'packages' : peek === 'account' && ACCOUNTS ? 'account' : 'consent');
	// An earlier version of the text was accepted on this install: the text changed, the rest is known.
	const again = app.consent !== null && !peek;
	let accepting = $state(false);
	let problem = $state('');

	const system = $derived(app.system!);
	const flags = $derived(ux.flags);
	// Pro reads the machine like an instrument; the others leave out instruction sets and architectures.
	const facts = $derived(
		rigFacts(system).map((f) => {
			if (flags.techDetails) return f;
			if (f.label === 'Processor') return { ...f, detail: f.detail.split(', ').filter((x) => /core|thread/.test(x)).join(', ') };
			if (f.label === 'System') return { ...f, detail: '' };
			if (f.label === 'Graphics') return { ...f, detail: f.detail.replace(/^(Metal|CUDA|Vulkan|CPU), /, '') };
			return f;
		})
	);
	const tier = $derived(tierOf(system));
	const budget = $derived(budgetOf(system));
	// Chat first, then the other families Rigbrew can run today, then download-only ones.
	const rank = (p: (typeof app.packages)[number]) => (p.modality === 'chat' ? 0 : familyOf(p).runs ? 1 : 2);
	const runnable = $derived(app.packages.filter((p) => p.recommendation.verdict === 'run').sort((a, b) => rank(a) - rank(b)));
	// Builds with accounts ask who you are right after the terms; it can be skipped.
	type Step = 'consent' | 'account' | 'scan' | 'packages';
	const steps: Step[] = again ? [] : ACCOUNTS ? ['consent', 'account', 'scan', 'packages'] : ['consent', 'scan', 'packages'];
	const stepNames = { consent: 'Terms', account: 'Account', scan: 'This machine', packages: 'Models' };

	// The scan itself is instant; the reveal is paced so each reading can be seen.
	let revealed = $state(0);
	const scanned = $derived(revealed >= facts.length);

	async function accept() {
		accepting = true;
		problem = '';
		try {
			await app.accept();
			if (again) {
				app.enter();
				return;
			}
			if (ACCOUNTS) {
				step = 'account';
				app.wakeAccount();
				app.track('account_step_shown', {});
			} else toScan();
		} catch (e) {
			problem = e instanceof Error ? e.message : String(e);
		} finally {
			accepting = false;
		}
	}

	function toScan() {
		if (step === 'account') app.track('account_step_done', {});
		step = 'scan';
		runScan();
	}

	let timers: ReturnType<typeof setTimeout>[] = [];

	function runScan() {
		revealed = 0;
		const still = window.matchMedia('(prefers-reduced-motion: reduce)').matches;
		facts.forEach((_, i) => {
			timers.push(setTimeout(() => (revealed = i + 1), still ? 0 : 420 * (i + 1)));
		});
	}

	onMount(() => {
		if (step === 'consent') app.track('consent_shown', { reason: again ? 'changed' : 'first' });
		if (step === 'scan') runScan();
		if (step === 'account') app.wakeAccount();
		return () => timers.forEach(clearTimeout);
	});
</script>

<div class="welcome scroll">
	<div class="drag" data-tauri-drag-region></div>

	<main>
		<header class="top">
			<Mark size={20} />
			<span class="wordmark" aria-label="Rigbrew">rig<span>brew</span></span>
			{#if steps.length > 0}
				<ol class="steps" aria-label="Steps">
					{#each steps as name, i (name)}
						<li class:now={step === name} class:done={steps.indexOf(step) > i} aria-current={step === name ? 'step' : undefined}><span class="num">{i + 1}</span>{stepNames[name]}</li>
					{/each}
				</ol>
			{/if}
		</header>

		{#if step === 'consent'}
			<h1>{again ? 'The terms changed' : 'Before you start'}</h1>
			<p class="intro">
				{#if again}The text you accepted earlier has a new version: what Rigbrew collects now comes in two parts. Read it and accept again to keep using Rigbrew.{:else}Rigbrew runs AI models on your own {machine}. Your prompts are not sent to any model provider. Rigbrew itself collects data in two parts.{/if}
			</p>

			<article class="legal scroll selectable">
				<p class="placeholder">Draft. The final legal text is not reviewed yet.</p>
				<h2 class="label">What you write and what the models answer, if you agree</h2>
				<p>
					The full text of your prompts and of the answers, and the descriptions of pictures you ask for, linked to your Rigbrew account (anonymous until you sign in).
					Pictures, sound and video stay on this {machine}. It is used to see where the models fail, to make them better, and to train and evaluate models.
					Do not type anything you would not want stored on a server, including personal information about other people.
				</p>
				<h2 class="label">How the app is used, always</h2>
				<p>
					Screens opened, main buttons used, models downloaded, started and removed, how fast they ran, errors, your operating system, a hardware class, the app version and a random installation number.
					It never contains what you type or what the models answer, is not linked to your account, and is sent whether or not you agree to the first part.
				</p>
				<p class="also">Settings, under Privacy and data, shows everything waiting to be sent. You can withdraw there at any time: content not yet sent is removed and no new content is collected; basic usage data continues.</p>
				<p class="version num">Consent text version {app.requiredConsent}</p>
			</article>

			{#if problem}<p class="problem" role="alert">Could not save your answer. {problem}</p>{/if}
			<p class="actions">
				<button class="btn primary large" onclick={accept} disabled={accepting}>Accept and continue</button>
				<span class="hint">
					You confirm you are at least 18 and agree to the first part, the
					<button class="link" onclick={() => guide.openLegal('privacy', 'consent')}>Privacy Policy</button> and the
					<button class="link" onclick={() => guide.openLegal('terms', 'consent')}>Terms of Use</button>. Rigbrew does not work without this.
				</span>
			</p>
		{:else if step === 'account'}
			<h1>Sign in, or skip for now</h1>
			<p class="intro">An account keeps your chats and usage under one name on every computer you use, and a few models can only be downloaded with one. Chat, pictures and everything else work without it.</p>
			<AccountWelcome ondone={toScan} />
		{:else if step === 'scan'}
			<h1 aria-live="polite">{scanned ? (flags.techDetails ? `${tier.label}.` : `${system.cpu.brand}: ready.`) : `Reading this ${machine}`}</h1>
			<p class="intro">{scanned ? (flags.techDetails ? tier.line : machineLine(system)) : 'Rigbrew checks how much memory and disk space there is, to pick models that fit. Nothing is sent anywhere for this.'}</p>

			<ol class="readout" aria-live="polite">
				{#each facts as fact, i (fact.label + i)}
					<li class:shown={i < revealed} class:reading={i === revealed}>
						<span class="label">{fact.label}</span>
						<span class="lead" aria-hidden="true"></span>
						<span class="value">{#if i < revealed}{fact.value}{:else if i === revealed}<span class="cursor"></span>{/if}</span>
						<span class="detail num">{i < revealed ? fact.detail : ''}</span>
					</li>
				{/each}
			</ol>

			<div class="budget" class:shown={scanned}>
				<p class="figure"><span class="label">Models may use</span> <strong class="num">{size(budget.mb)}</strong> <span class="muted">of {size(budget.of)}{flags.techDetails ? `, on the ${budget.where}` : ' memory'}</span></p>
				<div class="rail"><Track budget={budget.mb} height={30} label="Memory models may use: {size(budget.mb)}" /></div>
				<p class="muted note">{flags.techDetails ? 'The rest stays with the system. A build is recommended only when it fits inside this track.' : `The rest stays with the system and your other apps. Rigbrew recommends only models that fit in this space.`}</p>
			</div>

			<p class="actions" class:hidden={!scanned}>
				<button class="btn primary large" onclick={() => (step = 'packages')} disabled={!scanned}>See what runs here</button>
			</p>
		{:else}
			{#if flags.techDetails}
				<h1>One chat, several models</h1>
				<p class="intro">{FAMILIES_EXPLAINED}</p>
				<p class="intro second">These builds were picked for your {system.cpu.brand}. Install what you want now; the rest stays in Models.</p>
			{:else}
				<h1>Pick what to install</h1>
				<p class="intro">A model is what answers you, and each one does one thing. These suit your {system.cpu.brand}. Start with Chat; the rest can wait in Models.</p>
			{/if}

			<ul class="picks">
				{#each runnable as pkg (pkg.id)}
					{@const family = familyOf(pkg)}
					{@const rec = pkg.recommendation}
					<li class:plain={!flags.techDetails}>
						<span class="pname">{nameOf(pkg, flags)}</span>
						{#if flags.techDetails}
							<span class="num build">{rec.verdict === 'run' ? variantName(rec.variant) : ''}</span>
							<span class="tag">{family.does}</span>
						{/if}
						<span class="about">{flags.techDetails ? `${family.can}.` : plainDoes(pkg)}{#if !family.runs}&nbsp;{flags.techDetails ? RUNS_LATER : 'Download only for now.'}{/if}{#if pkg.credit}&nbsp;<strong>{pkg.credit}.</strong>{/if}</span>
						<span class="num mem">{rec.verdict === 'run' ? (flags.techDetails ? `${size(rec.memory_mb)} of ${size(rec.placement === 'accelerator' ? system.accel_budget_mb : system.cpu_budget_mb)}` : `uses ${size(rec.memory_mb)} of memory`) : ''}</span>
						<span class="control">
							{#if pkg.installed.length > 0}<span class="tag accent">installed</span>{:else}<InstallControl {pkg} primary={family.runs && pkg.modality === 'chat'} from="first_run" />{/if}
						</span>
					</li>
				{/each}
			</ul>

			<p class="actions">
				<button class="btn large" class:primary={app.chatPackages.length > 0} onclick={() => app.enter(undefined, true)}>
					{app.chatPackages.length > 0 ? 'Start chatting' : 'Continue without a model'}
				</button>
				{#if app.chatPackages.length === 0}<span class="hint">Downloads keep running; the chat opens when a chat model is installed.</span>{/if}
			</p>
		{/if}
	</main>
</div>

<LegalSheet />

<style>
	.welcome {
		height: 100%;
		background: var(--bg);
	}

	.drag {
		position: fixed;
		inset: 0 0 auto;
		height: 40px;
	}

	/* One centred column: the same width as before, in the middle of any window. */
	main {
		width: 100%;
		max-width: 760px;
		margin: 0 auto;
		padding: clamp(48px, 9vh, 96px) clamp(24px, 4vw, 40px) 40px;
		display: flex;
		flex-direction: column;
		align-items: flex-start;
	}

	.top {
		display: flex;
		align-items: center;
		gap: 8px;
		width: 100%;
		padding-bottom: 12px;
		border-bottom: 1px solid var(--line);
	}

	.wordmark {
		font: 600 13px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.wordmark span {
		font-weight: 300;
	}

	.steps {
		display: flex;
		gap: 16px;
		margin-left: auto;
		font-size: 12px;
		color: var(--faint);
	}

	.steps li {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.steps .num {
		font-size: 11px;
	}

	.steps .done {
		color: var(--muted);
	}

	.steps .now {
		color: var(--ink);
		font-weight: 600;
	}

	h1 {
		margin-top: 28px;
		font-size: 22px;
	}

	.intro {
		margin-top: 8px;
		max-width: 60ch;
		color: var(--muted);
	}

	.intro.second {
		margin-top: 4px;
	}

	.legal {
		width: 100%;
		max-height: min(46vh, 380px);
		margin-top: 18px;
		padding: 12px 14px;
		border: 1px solid var(--line);
		border-radius: var(--r);
		background: var(--well);
		font-size: 12.5px;
	}

	.legal h2 {
		margin: 12px 0 4px;
		font-weight: 400;
	}

	.legal p + h2 {
		margin-top: 14px;
	}

	.legal .also {
		margin-top: 12px;
		color: var(--muted);
	}

	.placeholder {
		color: var(--warn);
	}

	.version {
		margin-top: 12px;
		font-size: 11.5px;
		color: var(--muted);
	}

	.actions {
		display: flex;
		align-items: center;
		gap: 12px;
		margin-top: 18px;
		transition: opacity 0.3s var(--ease);
	}

	.actions.hidden {
		opacity: 0;
	}

	.hint {
		max-width: 56ch;
		font-size: 12px;
		color: var(--muted);
	}

	/* The readout: one line per reading, drawn as it is taken. */
	.readout {
		width: 100%;
		margin-top: 20px;
		border-top: 1px solid var(--line);
	}

	.readout li {
		display: grid;
		grid-template-columns: 84px 28px minmax(0, auto) minmax(0, 1fr);
		align-items: center;
		gap: 10px;
		height: 30px;
		border-bottom: 1px solid var(--line-soft);
		white-space: nowrap;
	}

	.readout .label {
		color: var(--faint);
		transition: color 0.2s;
	}

	.readout .shown .label,
	.readout .reading .label {
		color: var(--muted);
	}

	.lead {
		height: 1px;
		background: var(--line);
		transform-origin: left;
		transform: scaleX(0);
		transition: transform 0.35s var(--ease);
	}

	.shown .lead,
	.reading .lead {
		transform: scaleX(1);
	}

	.shown .lead {
		background: var(--muted);
	}

	.value {
		font: 600 13px/1 var(--rb-display);
		letter-spacing: -0.045em;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.shown .value {
		animation: appear 0.2s var(--ease) both;
	}

	.cursor {
		display: inline-block;
		width: 7px;
		height: 13px;
		background: var(--accent);
		animation: blink 0.5s steps(2, jump-none) infinite;
	}

	.detail {
		font-size: 11.5px;
		color: var(--muted);
		text-align: right;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.budget {
		width: 100%;
		margin-top: 20px;
		opacity: 0;
		transition: opacity 0.4s var(--ease);
	}

	.budget.shown {
		opacity: 1;
	}

	.figure {
		display: flex;
		align-items: baseline;
		gap: 8px;
	}

	.figure strong {
		font-size: 22px;
		font-weight: 500;
		letter-spacing: -0.03em;
		line-height: 1;
	}

	/* The track is drawn from the left once the readings are in. */
	.rail {
		margin-top: 6px;
		clip-path: inset(0 100% 0 0);
		transition: clip-path 0.9s var(--ease) 0.15s;
	}

	.shown .rail {
		clip-path: inset(0 0 0 0);
	}

	.note {
		margin-top: 8px;
		font-size: 12px;
	}

	.picks {
		width: 100%;
		margin-top: 18px;
		border-top: 1px solid var(--line);
	}

	.picks li {
		display: grid;
		grid-template-columns: 96px 34px 76px minmax(0, 1fr) auto;
		grid-template-areas: 'name build tag mem control' 'about about about about control';
		align-items: center;
		gap: 2px 10px;
		padding: 8px 0 9px;
		border-bottom: 1px solid var(--line-soft);
	}

	.pname {
		grid-area: name;
		font: 600 13px/1 var(--rb-display);
		letter-spacing: -0.045em;
		white-space: nowrap;
	}

	.build {
		grid-area: build;
		font-size: 12px;
		color: var(--muted);
	}

	.picks .tag {
		grid-area: tag;
		justify-self: start;
	}

	.mem {
		grid-area: mem;
		font-size: 12px;
		color: var(--muted);
	}

	.about {
		grid-area: about;
		font-size: 12.5px;
		color: var(--muted);
	}

	.control {
		grid-area: control;
		justify-self: end;
	}

	/* Plain names are longer: the name takes the first line, what it does the second. */
	.picks li.plain {
		grid-template-columns: minmax(0, auto) minmax(0, 1fr) auto;
		grid-template-areas: 'name mem control' 'about about control';
	}

	.picks li.plain .mem {
		justify-self: start;
	}
</style>
