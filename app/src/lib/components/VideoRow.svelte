<script lang="ts">
	import { copyText, saveVideo, videoFile } from '#lib/api.ts';
	import { chat, clipUrl, videoSizes, videoSizesOf, type VideoJob } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { elapsed, variantName } from '#lib/format.ts';
	import { app } from '#lib/state.svelte.ts';
	import { versionLabel } from '#lib/words.ts';
	import ErrorLine from './ErrorLine.svelte';
	import Icon from './Icon.svelte';

	// A clip in the transcript, the same shape as a picture card. A clip takes minutes, so while it
	// is being made the card says what the engine is doing (reading the description, the steps of
	// drawing the frames, putting them together) with the step count and the time so far.
	let { job, now }: { job: VideoJob; now: number } = $props();

	const flags = $derived(ux.flags);
	const pkg = $derived(app.packages.find((p) => p.id === job.package));
	const waited = $derived(job.waited ?? Math.max(0, (now - job.startedAt) / 1000));
	const shape = $derived([...Object.values(videoSizesOf(pkg)), ...Object.values(videoSizes)].find((s) => s.width === job.width && s.height === job.height)?.label ?? `${job.width}×${job.height}`);
	// A clip with a sound track starts on a click, with its sound; a silent one plays by itself.
	const sound = $derived(Boolean(pkg?.defaults?.audio));
	const length = $derived(`${(job.frames / 24).toFixed(1)} s`);
	const share = $derived(job.step !== null && job.totalSteps ? job.step / job.totalSteps : 0);
	let open = $state(false);
	let copied = $state(false);
	let saved = $state('');
	let problem = $state('');

	const doing = $derived.by(() => {
		switch (job.stage) {
			case 'loading':
			case 'preparing':
			case 'waiting':
			case 'queued':
				return flags.techDetails ? 'loading' : `moving ${job.packageName} into memory`;
			case 'conditioning':
				return flags.techDetails ? 'encoding the prompt' : 'reading the description';
			case 'sampling':
				return job.step !== null && job.totalSteps
					? `${flags.techDetails ? 'sampling' : 'drawing the frames'}, step ${job.step + (job.step < job.totalSteps ? 1 : 0)} of ${job.totalSteps}`
					: flags.techDetails
						? 'sampling'
						: 'drawing the frames';
			case 'vae_decode':
			case 'encoding':
				return flags.techDetails ? 'decoding' : 'putting the clip together';
			default:
				return flags.techDetails ? job.stage || 'working' : 'working';
		}
	});

	let src = $state('');
	let missing = $state(false);
	$effect(() => {
		const file = job.file;
		if (job.state !== 'done' || !file) {
			missing = job.state === 'done';
			return;
		}
		let gone = false;
		clipUrl(file)
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
		if (!job.file) return;
		problem = '';
		const words = job.prompt.toLowerCase().replace(/[^a-z0-9]+/g, '-').replace(/^-|-$/g, '').slice(0, 40);
		try {
			const how = await saveVideo(`${words || 'clip'}-${job.seed}.webm`, await videoFile(job.file));
			if (how === 'cancelled') return;
			app.track('video_saved', { package: job.package, via: how === 'saved' ? 'dialog' : 'download' });
			saved = how === 'saved' ? 'Saved' : 'Downloaded';
			setTimeout(() => (saved = ''), 1600);
		} catch (e) {
			problem = e instanceof Error ? e.message : String(e);
		}
	}
</script>

<div class="tool" role="group" aria-label="Video: {job.prompt}" style:--w={job.width} style:--h={job.height}>
	<div class="line">
		<button class="call" class:num={flags.techDetails} onclick={() => (open = !open)} aria-expanded={open} title={open ? 'Hide the description' : 'Show the description it was made from'}>
			<span class="caret" class:open><Icon name="chevron" size={12} /></span>
			{#if flags.techDetails}
				<span class="fn">/video</span>
				<span class="arrow">→</span>
				<b>{job.packageName}</b>
				{#if job.variant}<span class="sep">{variantName(job.variant)}</span>{/if}
				<span class="sep">{job.width}×{job.height}</span>
				<span class="sep">{job.frames} frames</span>
				<span class="sep">{job.steps} steps</span>
			{:else}
				<b>Video</b>
				<span class="sep">made by {job.packageName}{#if flags.versionNames && pkg && job.variant}&nbsp;· {versionLabel(pkg, job.variant, flags)}{/if}</span>
				{#if flags.versionNames}<span class="sep">{shape}, {length}</span>{/if}
			{/if}
			{#if job.state === 'waiting'}
				<span class="sep live num" aria-live="polite"><span class="dot"></span>{doing} · {elapsed(waited)}</span>
			{:else if job.state === 'done' && job.seconds !== undefined}
				<span class="sep num">{elapsed(job.seconds)}</span>
			{:else if job.state === 'stopped'}
				<span class="sep">stopped after {elapsed(waited)}</span>
			{:else if job.state === 'failed'}
				<span class="sep failed">{flags.techDetails ? 'failed' : 'not made'}</span>
			{/if}
		</button>
		{#if job.state === 'waiting'}
			<button class="btn quiet" onclick={() => chat.stopVideo(job)} disabled={!job.jobId} title="Stop making this clip"><Icon name="stop" size={13} /> Stop</button>
		{/if}
	</div>

	{#if open}
		<dl class="detail selectable">
			<div><dt>{flags.techDetails ? 'prompt' : 'described as'}</dt><dd>{job.prompt}</dd></div>
			{#if flags.techDetails}<div><dt>seed</dt><dd class="num">{job.seed}</dd></div>{/if}
		</dl>
	{/if}

	{#if job.state === 'failed'}
		<ErrorLine error={job.error} lead="The clip was not made." where="video" />
	{:else if job.state === 'waiting'}
		<div class="frame working">
			<div class="bar" style:--share={share} aria-hidden="true"></div>
			<p class="note">
				{flags.techDetails
					? `${job.packageName} renders ${job.frames} frames in ${job.steps} steps, then decodes them. A few minutes on a fast Mac.`
					: `${job.packageName} is making a ${length} clip. This takes a few minutes; it keeps going if you open another chat.`}
				{#if waited > 600} Still working; it takes longer while the machine is busy.{/if}
			</p>
		</div>
	{:else if job.state === 'done' && src}
		<div class="frame">
			<!-- svelte-ignore a11y_media_has_caption -->
			<video {src} width={job.width} height={job.height} controls loop muted={!sound} autoplay={!sound} playsinline aria-label={job.prompt}></video>
		</div>
		{#if pkg?.credit}<p class="credit">{pkg.credit}</p>{/if}
	{:else if job.state === 'done' && missing}
		<p class="gone">This clip is no longer on this machine.</p>
	{/if}

	{#if job.state !== 'waiting'}
		<div class="actions">
			{#if job.state === 'done' && !missing}
				<button class="btn quiet" onclick={save} title="Save a copy of this clip as a WebM file"><Icon name={saved ? 'check' : 'download'} size={14} /> {saved || 'Save'}</button>
			{/if}
			<button class="btn quiet" onclick={copy} title="Copy the description to the clipboard"><Icon name={copied ? 'check' : 'copy'} size={14} /> {copied ? 'Copied' : flags.techDetails ? 'Copy prompt' : 'Copy description'}</button>
		</div>
		{#if problem}<p class="problem selectable" role="alert">Could not save the clip. {problem}</p>{/if}
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
		font-size: 12.5px;
		line-height: 18px;
		color: var(--muted);
		text-align: left;
	}

	.call.num {
		font-size: 12px;
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

	.gone {
		padding: 8px 10px;
		border-left: 2px solid var(--line);
		font-size: 12.5px;
		color: var(--muted);
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

	/* Frames on a strip: the ruled plate of the picture card, turned sideways, filling as steps finish. */
	.frame.working {
		display: grid;
		place-items: end start;
		background-image: linear-gradient(to right, var(--line-soft) 1px, transparent 1px);
		background-size: 12px 100%;
	}

	.bar {
		position: absolute;
		inset: auto 0 0 0;
		height: 3px;
		background: linear-gradient(to right, var(--accent) calc(var(--share) * 100%), transparent 0);
		transition: background var(--quick);
	}

	.note {
		position: relative;
		margin: 10px 10px 14px;
		padding: 6px 8px;
		max-width: 38ch;
		background: var(--bg);
		box-shadow: 0 0 0 1px var(--line);
		border-radius: 3px;
		font-size: 12px;
		color: var(--muted);
	}

	video {
		display: block;
		width: 100%;
		height: 100%;
		object-fit: contain;
		background: var(--well);
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

	.credit {
		margin-top: 6px;
		font-size: 11.5px;
		color: var(--muted);
	}
</style>
