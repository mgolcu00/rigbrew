<script lang="ts">
	import { isTauri, openExternal, type AgentTask } from '#lib/api.ts';
	import { code } from '#lib/code.svelte.ts';
	import { ago } from '#lib/format.ts';
	import { remembered } from '#lib/remember.ts';
	import Icon from './Icon.svelte';
	import Shot from './Shot.svelte';

	// Next to a Code task: what the task made, as the model sees it. The running app in a frame
	// (at a phone's, a tablet's or a desktop's width), the errors of the page it last looked at
	// under it, and the last picture of the phone's screen when the task works on a phone app.
	//
	// The frame is another origin and is sandboxed: it cannot reach the app around it or move
	// this window to another page.

	let { task }: { task: AgentTask } = $props();

	const widths = { phone: 390, tablet: 820, desktop: 1280 } as const;
	type Size = keyof typeof widths;
	const sizes: { id: Size; name: string }[] = [
		{ id: 'phone', name: 'Phone' },
		{ id: 'tablet', name: 'Tablet' },
		{ id: 'desktop', name: 'Desktop' }
	];

	let size = $state<Size>((['phone', 'tablet', 'desktop'] as const).find((s) => s === remembered.get('code.preview.size')) ?? 'desktop');
	/** Counts reloads: a new number makes a new frame. */
	let reloads = $state(0);
	let width = $state(0);
	/** Which of the two is shown when the task has both a page and a phone. */
	let picked = $state<'page' | 'phone' | null>(null);

	const running = $derived(task.app ?? null);
	const page = $derived(task.page ?? null);
	const device = $derived(task.device ?? null);
	const view = $derived(picked && (picked === 'page' ? running : device) ? picked : running ? 'page' : 'phone');
	const scale = $derived(width ? Math.min(1, width / widths[size]) : 1);
	const found = $derived((page?.errors.length ?? 0) + (page?.failed.length ?? 0));
	const address = $derived(running ? running.url.replace(/^https?:\/\//, '').replace(/\/$/, '') : '');

	function setSize(next: Size) {
		size = next;
		remembered.set('code.preview.size', next);
	}

	// Reload shows the page afresh and, in the app, looks at it again the way the model does,
	// so the errors under the frame are those of the page as it is now.
	async function reload() {
		reloads++;
		if (isTauri && running?.alive) await code.lookAgain(size);
	}
</script>

<aside class="preview" aria-label="Preview">
	<header>
		{#if running && device}
			<div class="seg" role="group" aria-label="What to show">
				<button class:on={view === 'page'} aria-pressed={view === 'page'} onclick={() => (picked = 'page')}>Page</button>
				<button class:on={view === 'phone'} aria-pressed={view === 'phone'} onclick={() => (picked = 'phone')}>Phone</button>
			</div>
		{:else}
			<h2>Preview</h2>
		{/if}
		{#if view === 'page' && address}<code class="where" title={running?.command}>{address}</code>{/if}
		<span class="grow"></span>
		{#if view === 'page' && running}
			<button class="icon" onclick={reload} disabled={code.looking} title="Reload" aria-label="Reload"><Icon name="redo" size={15} /></button>
			<button class="icon" onclick={() => openExternal(running.url)} title="Open in the browser" aria-label="Open in the browser"><Icon name="external" size={15} /></button>
			{#if running.alive}
				<button class="icon" onclick={() => code.stopApp()} title="Stop the app" aria-label="Stop the app"><Icon name="stop" size={15} /></button>
			{/if}
		{:else if view === 'phone'}
			<button class="icon" onclick={() => code.deviceShot()} disabled={code.looking} title="Take a new picture" aria-label="Take a new picture"><Icon name="redo" size={15} /></button>
		{/if}
		<button class="icon" onclick={() => code.setPreview(false)} title="Hide the preview" aria-label="Hide the preview"><Icon name="close" size={15} /></button>
	</header>

	{#if view === 'page' && running?.alive}
		<div class="seg sizes" role="group" aria-label="Width">
			{#each sizes as s (s.id)}
				<button class:on={size === s.id} aria-pressed={size === s.id} onclick={() => setSize(s.id)}>{s.name}</button>
			{/each}
			<span class="px">{widths[size]} px</span>
		</div>
	{/if}

	<div class="stage" bind:clientWidth={width}>
		{#if view === 'page' && running}
			{#if running.alive}
				<div class="fit" style:width="{widths[size] * scale}px">
					{#key reloads}
						<iframe
							title="The running app"
							src={running.url}
							sandbox="allow-scripts allow-same-origin allow-forms allow-modals"
							style:width="{widths[size]}px"
							style:height="{100 / scale}%"
							style:transform="scale({scale})"
						></iframe>
					{/key}
				</div>
			{:else}
				<div class="quiet">
					<p>The app is not running.</p>
					{#if running.lines.length}<pre class="selectable">{running.lines.join('\n')}</pre>{/if}
				</div>
			{/if}
		{:else if device}
			<div class="phone">
				<Shot task={task.id} name={device.shot} alt="The screen of {device.name}" />
			</div>
		{/if}
	</div>

	{#if view === 'page' && running}
		<details class="console" open={found > 0}>
			<summary>
				Console
				{#if page}<span class="n" class:bad={found > 0}>{found}</span>{/if}
				<span class="grow"></span>
				{#if code.looking}<span class="when">Looking…</span>{:else if page}<span class="when">{ago(page.at_ms)}</span>{/if}
			</summary>
			{#if !page}
				<p class="none">Not looked at yet</p>
			{:else if !found}
				<p class="none">{page.rendered ? 'No errors' : 'Not rendered: only the HTML was read'}</p>
			{:else}
				<ul class="selectable">
					{#each page.errors as line, i (i)}<li class="bad">{line}</li>{/each}
					{#each page.failed as line, i (i)}<li>{line}</li>{/each}
				</ul>
			{/if}
			{#if page?.note}<p class="none">{page.note}</p>{/if}
		</details>
	{:else if device}
		<p class="foot">{device.name}<span class="grow"></span><span class="when">{ago(device.at_ms)}</span></p>
	{/if}
</aside>

<style>
	.preview {
		height: 100%;
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		border-left: 1px solid var(--line-soft);
		background: var(--bg);
		font-family: var(--rb-text);
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 6px;
		min-height: 44px;
		padding: 6px 8px 6px 14px;
		border-bottom: 1px solid var(--line-soft);
	}

	h2 {
		font: 600 13px/1.3 var(--rb-text);
		letter-spacing: 0;
	}

	.where {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 12px var(--rb-data);
		color: var(--muted);
	}

	.grow {
		flex: 1;
	}

	.icon {
		flex: none;
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		border-radius: 7px;
		color: var(--muted);
	}

	.icon:hover:not(:disabled) {
		background: var(--hover);
		color: var(--ink);
	}

	.icon:disabled {
		opacity: 0.4;
	}

	.seg {
		display: flex;
		align-items: center;
		gap: 2px;
	}

	.seg button {
		height: 24px;
		padding: 0 9px;
		border-radius: 6px;
		font: 500 12px/1 var(--rb-text);
		color: var(--muted);
	}

	.seg button:hover {
		color: var(--ink);
	}

	.seg button.on {
		background: var(--hover);
		color: var(--ink);
	}

	.sizes {
		flex: none;
		padding: 6px 12px;
		border-bottom: 1px solid var(--line-soft);
	}

	.px {
		margin-left: auto;
		font: 11px var(--rb-data);
		color: var(--faint);
	}

	.stage {
		flex: 1;
		min-height: 0;
		overflow: hidden;
		background: var(--hover);
	}

	.fit {
		height: 100%;
		margin: 0 auto;
		overflow: hidden;
	}

	iframe {
		display: block;
		border: 0;
		transform-origin: 0 0;
		background: white;
	}

	.quiet {
		height: 100%;
		display: grid;
		align-content: center;
		gap: 10px;
		padding: 20px;
		color: var(--muted);
		font-size: 13px;
		text-align: center;
	}

	.quiet pre {
		max-height: 50%;
		overflow: auto;
		padding: 10px 12px;
		border-radius: 8px;
		background: var(--bg);
		font: 11px/1.5 var(--rb-data);
		text-align: left;
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.phone {
		height: 100%;
		padding: 14px;
	}

	.console {
		flex: none;
		max-height: 34%;
		overflow: auto;
		border-top: 1px solid var(--line-soft);
	}

	.console summary {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 14px;
		font: 500 12px/1.3 var(--rb-text);
		color: var(--muted);
		cursor: pointer;
		list-style: none;
	}

	.console summary::-webkit-details-marker {
		display: none;
	}

	.n {
		font: 11px var(--rb-data);
		color: var(--faint);
	}

	.n.bad {
		color: var(--warn);
	}

	.when {
		font-size: 11px;
		font-weight: 400;
		color: var(--faint);
	}

	.console ul {
		list-style: none;
		padding: 0 14px 10px;
		display: grid;
		gap: 4px;
	}

	.console li {
		font: 11px/1.45 var(--rb-data);
		color: var(--muted);
		overflow-wrap: anywhere;
	}

	.console li.bad {
		color: var(--ink);
	}

	.none {
		padding: 0 14px 10px;
		font-size: 12px;
		color: var(--faint);
	}

	.foot {
		flex: none;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 8px 14px;
		border-top: 1px solid var(--line-soft);
		font-size: 12px;
		color: var(--muted);
	}
</style>
