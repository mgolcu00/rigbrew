<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { artifactFrame, copyText, saveText } from '#lib/api.ts';
	import { fileNameOf, kindName, pageDocument, svgDataUrl, textOf, type Artifact } from '#lib/artifacts.ts';
	import { app } from '#lib/state.svelte.ts';
	import Icon from './Icon.svelte';

	// One artifact, next to the conversation: the result as it looks (a page in a sandboxed
	// frame, a drawing as an image, a document as text) or its code, with Copy and Save, and the
	// earlier versions when the model wrote it more than once.
	//
	// Nothing the model wrote becomes part of the app's own page. A page runs in a frame with an
	// origin of its own that can load nothing from anywhere; a drawing is an <img>.

	let {
		artifact,
		versions,
		onpick,
		onclose,
		document: documentBody
	}: {
		artifact: Artifact;
		/** Every version of this artifact, oldest first; `artifact` is one of them. */
		versions: Artifact[];
		onpick: (id: string) => void;
		onclose: () => void;
		/** Renders a Markdown document the way the conversation renders an answer. */
		document: Snippet<[string]>;
	} = $props();

	let view = $state<'preview' | 'code'>('preview');
	let copied = $state(false);
	let broken = $state(false);
	let codeBox = $state<HTMLPreElement>();
	/** Where pages are shown: the service's frame page, null without one, undefined until asked. */
	let frameUrl = $state<string | null | undefined>(undefined);
	/** How long the frame page gets to say it is there before the page is shown from its text instead. */
	const FRAME_WAIT_MS = 2500;

	const previews = $derived(artifact.kind === 'page' || artifact.kind === 'drawing' || artifact.kind === 'document');
	// A page or a drawing that is half written shows as code until it is whole.
	const shown = $derived(!previews || (artifact.writing && artifact.kind !== 'document') ? 'code' : view);
	const at = $derived(versions.findIndex((v) => v.id === artifact.id));
	const picture = $derived(artifact.kind === 'drawing' ? svgDataUrl(artifact.code) : null);
	const what = $derived(artifact.kind === 'code' && artifact.lang ? `${artifact.lang} code` : artifact.kind === 'page' ? 'Web page' : artifact.kind === 'diagram' ? 'Diagram, shown as code' : kindName(artifact.kind));

	$effect(() => {
		if (artifact.kind === 'page' && frameUrl === undefined) artifactFrame().then((url) => (frameUrl = url));
	});

	$effect(() => {
		artifact.id;
		broken = false;
		copied = false;
	});

	// While it is being written the code follows the newest line.
	$effect(() => {
		artifact.code;
		if (artifact.writing && shown === 'code') tick().then(() => codeBox?.scrollTo({ top: codeBox.scrollHeight }));
	});

	/**
	 * Hands the page to the service's frame page once that says it is ready; it writes it under
	 * its own policy. A frame that never answers (something refused to load it) is given up on.
	 */
	function framed(frame: HTMLIFrameElement, code: string) {
		let filled = false;
		const onmessage = (event: MessageEvent) => {
			if (filled || event.source !== frame.contentWindow || event.data !== 'rigbrew:frame-ready') return;
			filled = true;
			clearTimeout(timer);
			frame.contentWindow?.postMessage(pageDocument(code), '*');
		};
		const timer = setTimeout(() => {
			if (!filled) frameUrl = null;
		}, FRAME_WAIT_MS);
		window.addEventListener('message', onmessage);
		return {
			destroy() {
				clearTimeout(timer);
				window.removeEventListener('message', onmessage);
			}
		};
	}

	async function copy() {
		await copyText(textOf(artifact));
		copied = true;
		setTimeout(() => (copied = false), 1400);
	}

	async function save() {
		try {
			const how = await saveText(fileNameOf(artifact), textOf(artifact), artifact.kind === 'page' ? 'text/html' : artifact.kind === 'drawing' ? 'image/svg+xml' : 'text/plain');
			if (how === 'cancelled') return;
			app.track('artifact_saved', { kind: artifact.kind });
			app.say(how === 'saved' ? `Saved “${artifact.title}”.` : `“${artifact.title}” is in your downloads folder.`);
		} catch (e) {
			app.say(`Could not save it. ${e instanceof Error ? e.message : String(e)}`);
		}
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Escape' && !event.defaultPrevented) {
			event.preventDefault();
			event.stopPropagation();
			onclose();
		}
	}
</script>

<!-- svelte-ignore a11y_no_noninteractive_element_interactions -->
<aside class="piece" aria-label="{what}: {artifact.title}" {onkeydown}>
	<header>
		<button class="back" onclick={onclose} title="Back to the conversation"><Icon name="back" size={15} /> Back</button>
		<div class="name">
			<h2 title={artifact.title}>{artifact.title}</h2>
			<p>
				{#if artifact.writing}<span class="dot"></span>Writing…{:else}{what}{#if !artifact.closed}, not finished{/if}{/if}
			</p>
		</div>
		<span class="grow"></span>
		{#if versions.length > 1}
			<div class="versions" role="group" aria-label="Versions">
				<button onclick={() => onpick(versions[at - 1].id)} disabled={at <= 0} title="The version before" aria-label="The version before"><Icon name="back" size={13} /></button>
				<span class="num">{at + 1} of {versions.length}</span>
				<button onclick={() => onpick(versions[at + 1].id)} disabled={at >= versions.length - 1} title="The version after" aria-label="The version after"><Icon name="chevron" size={13} /></button>
			</div>
		{/if}
		{#if previews}
			<div class="switch" role="radiogroup" aria-label="Show">
				<button role="radio" aria-checked={shown === 'preview'} onclick={() => (view = 'preview')} disabled={artifact.writing && artifact.kind !== 'document'}>Preview</button>
				<button role="radio" aria-checked={shown === 'code'} onclick={() => (view = 'code')}>Code</button>
			</div>
		{/if}
		<button class="btn quiet" onclick={copy} disabled={!artifact.code} title="Copy the {artifact.kind === 'document' ? 'text' : 'code'}">{copied ? 'Copied' : 'Copy'}</button>
		<button class="btn" onclick={save} disabled={artifact.writing || !artifact.code} title="Save it as a file">Save</button>
		<button class="close" onclick={onclose} title="Close" aria-label="Close"><Icon name="close" size={15} /></button>
	</header>

	<div class="body">
		{#if shown === 'code'}
			<pre class="scroll selectable" bind:this={codeBox}><code>{artifact.code}</code></pre>
		{:else if artifact.kind === 'page'}
			{#if frameUrl === undefined}
				<p class="note">Opening…</p>
			{:else}
				{#key `${artifact.id}:${artifact.code.length}:${frameUrl}`}
					{#if frameUrl}
						<iframe title={artifact.title} sandbox="allow-scripts" referrerpolicy="no-referrer" src={frameUrl} use:framed={artifact.code}></iframe>
					{:else}
						<!-- No frame page (an older service, or the frame was refused): the page is shown from its text. Inside the app its scripts do not run then. -->
						<iframe title={artifact.title} sandbox="allow-scripts" referrerpolicy="no-referrer" srcdoc={pageDocument(artifact.code)}></iframe>
					{/if}
				{/key}
			{/if}
		{:else if artifact.kind === 'drawing'}
			{#if picture && !broken}
				<div class="easel scroll"><img src={picture} alt={artifact.title} onerror={() => (broken = true)} /></div>
			{:else}
				<p class="note">This drawing cannot be shown. Its code is under Code.</p>
			{/if}
		{:else}
			<div class="page scroll">{@render documentBody(artifact.code)}</div>
		{/if}
	</div>
</aside>

<style>
	.piece {
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		border-left: 1px solid var(--line-soft);
		background: var(--bg);
		font-family: var(--rb-text);
		animation: appear 0.14s var(--ease) both;
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 48px;
		padding: 6px 10px 6px 16px;
		border-bottom: 1px solid var(--line-soft);
	}

	.name {
		min-width: 0;
	}

	h2 {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 600 14px/1.3 var(--rb-text);
		letter-spacing: 0;
	}

	.name p {
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 12px;
		line-height: 1.3;
		color: var(--faint);
		white-space: nowrap;
	}

	.dot {
		width: 6px;
		height: 6px;
		background: var(--accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}

	.grow {
		flex: 1;
	}

	.back {
		display: none;
		align-items: center;
		gap: 4px;
		height: 28px;
		padding: 0 10px 0 6px;
		margin-left: -8px;
		border-radius: 8px;
		font-size: 13px;
		color: var(--muted);
	}

	.back:hover,
	.close:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.close {
		flex: none;
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		border-radius: 8px;
		color: var(--muted);
	}

	.versions {
		flex: none;
		display: flex;
		align-items: center;
		gap: 2px;
		font-size: 12px;
		color: var(--muted);
	}

	.versions button {
		display: grid;
		place-items: center;
		width: 22px;
		height: 24px;
		border-radius: 6px;
	}

	.versions button:hover:not(:disabled) {
		background: var(--hover);
		color: var(--ink);
	}

	.versions button:disabled {
		opacity: 0.35;
	}

	.switch {
		flex: none;
		display: flex;
		gap: 2px;
		padding: 2px;
		border-radius: 999px;
		background: var(--well);
	}

	.switch button {
		height: 24px;
		padding: 0 11px;
		border-radius: 999px;
		font: 500 12.5px/1 var(--rb-text);
		color: var(--muted);
	}

	.switch button[aria-checked='true'] {
		background: var(--bg);
		color: var(--ink);
		box-shadow: 0 1px 2px color-mix(in srgb, var(--ink) 12%, transparent);
	}

	.switch button:disabled {
		opacity: 0.45;
	}

	.body {
		flex: 1;
		min-height: 0;
		display: flex;
		flex-direction: column;
	}

	.body > * {
		flex: 1;
		min-height: 0;
	}

	pre {
		margin: 0;
		padding: 14px 18px 24px;
		font: 12.5px/1.6 var(--rb-data);
		tab-size: 2;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	/* A page expects a white sheet unless it says otherwise, whatever the app's own theme. */
	iframe {
		width: 100%;
		border: 0;
		background: #fff;
	}

	.easel {
		display: grid;
		place-items: center;
		padding: 20px;
		background: var(--well);
	}

	.easel img {
		max-width: 100%;
		max-height: 100%;
		min-width: min(240px, 100%);
		background: #fff;
		border-radius: 6px;
		box-shadow: 0 1px 2px color-mix(in srgb, var(--ink) 10%, transparent);
	}

	.page {
		padding: 18px 22px 32px;
	}

	.note {
		flex: none;
		padding: 18px 22px;
		color: var(--muted);
		font-size: 13.5px;
	}

	/* A narrow window: the artifact covers the conversation and Back returns to it. */
	@media (max-width: 1000px) {
		.piece {
			position: absolute;
			inset: 0;
			z-index: 5;
			border-left: 0;
		}

		.back {
			display: inline-flex;
		}

		.close {
			display: none;
		}
	}

	@media (max-width: 560px) {
		header {
			flex-wrap: wrap;
		}
	}
</style>
