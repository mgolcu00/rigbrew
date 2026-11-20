<script lang="ts">
	import { copyText, saveImage } from '#lib/api.ts';
	import { chat, pictureBase64, pictureUrl, sizes, type ImageJob } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { elapsed, seconds, variantName } from '#lib/format.ts';
	import { app } from '#lib/state.svelte.ts';
	import { versionLabel } from '#lib/words.ts';
	import ErrorLine from './ErrorLine.svelte';
	import Hint from './Hint.svelte';
	import Icon from './Icon.svelte';

	// A picture in the transcript. In Pro it reads as a tool call: one line that says what was
	// called and what answered (`generate_image → Crema 6B · 1024×1024 · 8 steps · 11.4 s`),
	// which opens to the prompt and the seed. The other levels say the same in fewer, plainer
	// words. The picture itself sits under the line: from memory right after it was made, or
	// read back from the file the service saved.
	let { job, now, onload }: { job: ImageJob; now: number; onload?: () => void } = $props();

	const flags = $derived(ux.flags);
	const pkg = $derived(app.packages.find((p) => p.id === job.package));
	const waited = $derived(job.waited ?? Math.max(0, (now - job.startedAt) / 1000));
	const call = $derived(job.via === 'tool' ? 'generate_image' : job.via === 'regenerate' ? 'regenerate' : '/image');
	const shape = $derived(Object.values(sizes).find((s) => s.width === job.width && s.height === job.height)?.label ?? `${job.width}×${job.height}`);
	let open = $state(false);
	let copied = $state(false);
	let saved = $state('');
	let problem = $state('');

	// A saved picture is fetched with the token and shown through an object URL.
	let src = $state('');
	let missing = $state(false);
	$effect(() => {
		if (job.base64) {
			src = `data:image/png;base64,${job.base64}`;
			return;
		}
		const id = job.imageId;
		if (job.state !== 'done' || !id) {
			missing = job.state === 'done';
			return;
		}
		let gone = false;
		pictureUrl(id)
			.then((url) => !gone && (src = url))
			.catch(() => !gone && (missing = true));
		return () => (gone = true);
	});

	async function copy() {
		await copyText(job.prompt);
		copied = true;
		setTimeout(() => (copied = false), 1600);
	}

	async function save() {
		problem = '';
		const words = job.prompt.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 40);
		try {
			const how = await saveImage(`${words || 'image'}-${job.seed}.png`, await pictureBase64(job));
			if (how === 'cancelled') return;
			app.track('image_saved', { package: job.package, via: how === 'saved' ? 'dialog' : 'download' });
			saved = how === 'saved' ? 'Saved' : 'Downloaded';
			setTimeout(() => (saved = ''), 1600);
		} catch (e) {
			problem = e instanceof Error ? e.message : String(e);
		}
	}
</script>

<div class="tool" role="group" aria-label="Image: {job.prompt}" style:--w={job.width} style:--h={job.height}>
	<div class="line">
		{#if flags.techDetails}
			<button class="call num" onclick={() => (open = !open)} aria-expanded={open} title={open ? 'Hide the prompt' : 'Show the prompt and the seed'}>
				<span class="caret" class:open><Icon name="chevron" size={12} /></span>
				<span class="fn">{call}</span>
				<span class="arrow">→</span>
				<b>{job.packageName}</b>
				{#if job.variant}<span class="sep">{variantName(job.variant)}</span>{/if}
				<span class="sep">{job.width}×{job.height}</span>
				<span class="sep">{job.steps} steps</span>
				{#if job.state === 'waiting'}
					<span class="sep live" aria-live="polite"><span class="dot"></span>{job.step != null && job.totalSteps ? `step ${Math.min(job.step + 1, job.totalSteps)}/${job.totalSteps}` : job.switching ? 'switching model' : 'generating'} {elapsed(waited)}</span>
				{:else if job.state === 'done' && job.seconds !== undefined}
					<span class="sep"><b>{seconds(job.seconds)}</b></span>
				{:else if job.state === 'stopped'}
					<span class="sep">stopped after {elapsed(waited)}</span>
				{:else if job.state === 'failed'}
					<span class="sep failed">failed</span>
				{/if}
			</button>
		{:else}
			<button class="call plain" onclick={() => (open = !open)} aria-expanded={open} title={open ? 'Hide the description' : 'Show the description it was made from'}>
				<span class="caret" class:open><Icon name="chevron" size={12} /></span>
				<b>Picture</b>
				<span class="sep">made by {job.packageName}{#if flags.versionNames && pkg && job.variant}&nbsp;· {versionLabel(pkg, job.variant, flags)}{/if}</span>
				{#if flags.versionNames}<span class="sep">{shape}</span>{/if}
				{#if job.via === 'tool' && job.askedBy}<span class="sep">asked for by {job.askedBy}</span>{/if}
				{#if job.state === 'waiting'}
					<span class="sep live num" aria-live="polite"><span class="dot"></span>{job.step != null && job.totalSteps ? `drawing, step ${Math.min(job.step + 1, job.totalSteps)} of ${job.totalSteps}` : job.switching ? 'loading the picture model' : 'drawing'} {elapsed(waited)}</span>
				{:else if job.state === 'done' && job.seconds !== undefined}
					<span class="sep num">{Math.round(job.seconds)} s</span>
				{:else if job.state === 'stopped'}
					<span class="sep">stopped</span>
				{:else if job.state === 'failed'}
					<span class="sep failed">not made</span>
				{/if}
			</button>
		{/if}
		{#if job.state === 'waiting'}
			<button class="btn quiet" onclick={() => chat.stop()} title="Stop waiting for this picture"><Icon name="stop" size={13} /> Stop{#if flags.keyHints}&nbsp;<kbd>esc</kbd>{/if}</button>
		{/if}
	</div>

	{#if open}
		<dl class="detail selectable">
			<div><dt>{flags.techDetails ? 'prompt' : 'described as'}</dt><dd>{job.prompt}</dd></div>
			{#if flags.techDetails}<div><dt>seed</dt><dd class="num">{job.seed}</dd></div>{/if}
			{#if job.askedBy}<div><dt>asked by</dt><dd>{job.askedBy}</dd></div>{/if}
		</dl>
	{/if}

	{#if job.state === 'failed'}
		<ErrorLine error={job.error} lead="The picture was not made." where="picture" onretry={app.imagePackage && !chat.busy ? () => chat.regenerate(job) : undefined} />
	{:else if job.state === 'waiting'}
		<div class="frame working" style:--share={job.step != null && job.totalSteps ? job.step / job.totalSteps : 0}>
			<p class="note">
				{#if job.switching}{flags.techDetails ? `The model in memory is unloaded and ${job.packageName} is loaded first. Tens of seconds on an idle machine.` : `${job.packageName} is moved into memory first. That takes a few seconds; drawing takes tens of seconds.`}{:else}{flags.techDetails ? `${job.packageName} is in memory and working.` : `${job.packageName} is drawing. This takes tens of seconds.`}{/if}
				{#if waited > 90} Still working; it takes longer while the machine is busy.{/if}
				{#if !flags.techDetails}<Hint topic="pictures" />{/if}
			</p>
		</div>
	{:else if job.state === 'done' && src}
		<div class="frame">
			<img {src} alt={job.prompt} width={job.width} height={job.height} {onload} />
		</div>
	{:else if job.state === 'done' && missing}
		<p class="gone">This picture is no longer on this machine. Try again makes a new one from the same description.</p>
	{/if}

	{#if job.state !== 'waiting'}
		<div class="actions">
			{#if job.state === 'done'}
				{#if !missing}<button class="btn quiet" onclick={save} title="Save a copy of this picture as a PNG file"><Icon name={saved ? 'check' : 'download'} size={14} /> {saved || 'Save'}</button>{/if}
			{/if}
			<button class="btn quiet" onclick={copy} title="Copy the description to the clipboard"><Icon name={copied ? 'check' : 'copy'} size={14} /> {copied ? 'Copied' : flags.techDetails ? 'Copy prompt' : 'Copy description'}</button>
			{#if job.state !== 'failed'}
				<button class="btn quiet" onclick={() => chat.regenerate(job)} disabled={chat.busy || !app.imagePackage} title={flags.techDetails ? 'Same prompt and size, new seed' : 'A new picture from the same description'}>
					<Icon name="redo" size={14} />
					{job.state === 'done' && flags.techDetails ? 'Regenerate' : 'Try again'}
				</button>
			{/if}
		</div>
		{#if problem}<p class="problem selectable" role="alert">Could not save the image. {problem}</p>{/if}
	{/if}
</div>

<style>
	.tool {
		--long: clamp(240px, 52vh, 420px);
		display: flex;
		flex-direction: column;
		gap: 6px;
		min-width: 0;
	}

	.line {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 24px;
	}

	.call {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 0 6px;
		min-width: 0;
		margin-left: -4px;
		padding: 3px 6px 3px 2px;
		border-radius: 3px;
		font-size: 12px;
		line-height: 18px;
		color: var(--muted);
		text-align: left;
	}

	.call:hover {
		background: var(--hover);
	}

	.call b {
		font-weight: 500;
		color: var(--ink);
	}

	.fn {
		color: var(--accent);
	}

	.call.plain {
		font-size: 12.5px;
	}

	.gone {
		padding: 8px 10px;
		border-left: 2px solid var(--line);
		font-size: 12.5px;
		color: var(--muted);
	}

	.arrow {
		color: var(--faint);
	}

	.sep::before {
		content: '·';
		margin-right: 6px;
		color: var(--faint);
	}

	.caret {
		display: grid;
		transition: transform var(--quick);
	}

	.caret.open {
		transform: rotate(90deg);
	}

	.live {
		color: var(--ink);
	}

	.dot {
		display: inline-block;
		width: 6px;
		height: 6px;
		margin-right: 6px;
		background: var(--accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}

	.failed {
		color: var(--fail);
	}

	.detail {
		display: grid;
		gap: 2px;
		padding: 6px 10px;
		border-left: 1px solid var(--line);
		margin-left: 3px;
		font-size: 12.5px;
	}

	.detail div {
		display: grid;
		grid-template-columns: 72px 1fr;
		gap: 8px;
	}

	.detail dt {
		font: 400 11px/18px var(--rb-label);
		text-transform: uppercase;
		color: var(--muted);
	}

	.detail dd {
		overflow-wrap: anywhere;
	}

	.frame {
		position: relative;
		width: min(100%, calc(var(--long) * min(1, var(--w) / var(--h))));
		aspect-ratio: var(--w) / var(--h);
		border-radius: 3px;
		overflow: hidden;
		background: var(--well);
		box-shadow: inset 0 0 0 1px var(--line);
	}

	/* Not here yet: a ruled plate with one line sweeping down it, like a scan head. */
	.frame.working {
		display: grid;
		place-items: end start;
		background-image: linear-gradient(to bottom, var(--line-soft) 1px, transparent 1px);
		background-size: 100% 8px;
	}

	.frame.working::before {
		content: '';
		position: absolute;
		inset: auto 0 0 0;
		height: 3px;
		background: linear-gradient(to right, var(--accent) calc(var(--share) * 100%), transparent 0);
	}

	.frame.working::after {
		content: '';
		position: absolute;
		inset: 0 0 auto;
		height: 1px;
		background: var(--accent);
		animation: sweep 2.6s var(--ease) infinite alternate;
	}

	@keyframes sweep {
		to {
			transform: translateY(calc(var(--long) * min(1, var(--h) / var(--w))));
		}
	}

	.note {
		position: relative;
		margin: 10px;
		padding: 6px 8px;
		max-width: 36ch;
		background: var(--bg);
		box-shadow: 0 0 0 1px var(--line);
		border-radius: 3px;
		font-size: 12px;
		color: var(--muted);
	}

	img {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: contain;
		animation: develop 0.4s var(--ease) both;
	}

	@keyframes develop {
		from {
			opacity: 0;
		}
	}

	.actions {
		display: flex;
		flex-wrap: wrap;
		gap: 2px;
		margin-left: -8px;
	}

	.problem {
		font-size: 12.5px;
	}
</style>
