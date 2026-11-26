<script lang="ts">
	import { tick, type Snippet } from 'svelte';
	import { agentFiles } from '#lib/api.ts';
	import { commands, fitted, matching, typing, withCommand, withMention, type Typing } from '#lib/code-compose.ts';

	// The box to type in on the Code screen. `/` at the start opens the commands (and the skills),
	// `@` opens the files of the folder; arrows move, Enter or Tab chooses, Esc closes. A picture
	// pasted or dropped here goes with the message. The row under the box is the screen's own.

	interface Choice {
		id: string;
		name: string;
	}

	let {
		draft = $bindable(''),
		pictures = $bindable<string[]>([]),
		placeholder = '',
		disabled = false,
		folder,
		skills,
		sees,
		choices,
		onsend,
		onapp,
		tools
	}: {
		draft?: string;
		/** PNG data addresses. */
		pictures?: string[];
		placeholder?: string;
		disabled?: boolean;
		folder: string;
		skills: { name: string; description: string }[];
		/** Whether the model takes pictures. */
		sees: boolean;
		/** What `/model` and `/mode` offer. */
		choices: { model: Choice[]; mode: Choice[] };
		onsend: () => void;
		/** A command the app does itself: `clear`, or a model or mode that was chosen. */
		onapp: (name: 'clear' | 'model' | 'mode', value?: string) => void;
		tools: Snippet;
	} = $props();

	let input = $state<HTMLTextAreaElement>();
	let caret = $state(0);
	/** Closed by Esc until the text changes. */
	let shut = $state(false);
	/** `/model` or `/mode` was chosen: its list is open. */
	let sub = $state<'model' | 'mode' | null>(null);
	let active = $state(0);
	let files = $state<string[]>([]);
	let dragging = $state(false);

	export function focus() {
		input?.focus();
	}

	const open = $derived<Typing>(disabled || shut ? null : typing(draft, caret));

	interface Item {
		key: string;
		label: string;
		hint: string;
		pick: () => void;
	}

	const items = $derived.by((): Item[] => {
		if (sub) {
			return choices[sub].map((c) => ({ key: c.id, label: c.name, hint: '', pick: () => app(sub ?? 'model', c.id) }));
		}
		if (open?.kind === 'slash') {
			return matching(commands(skills), open.query).map((c) => ({ key: c.name, label: `/${c.name}`, hint: c.hint, pick: () => command(c.name, c.kind) }));
		}
		if (open?.kind === 'at') {
			const at = open;
			return files.map((path) => ({ key: path, label: path, hint: '', pick: () => mention(at.start, path) }));
		}
		return [];
	});

	$effect(() => {
		void items.length;
		active = 0;
	});

	// The files for what follows the @, asked for a moment after the last key.
	$effect(() => {
		const at = open?.kind === 'at' ? open.query : null;
		if (at === null || !folder) {
			files = [];
			return;
		}
		let live = true;
		const wait = setTimeout(async () => {
			try {
				const found = await agentFiles(folder, at);
				if (live) files = found;
			} catch {
				if (live) files = [];
			}
		}, 90);
		return () => {
			live = false;
			clearTimeout(wait);
		};
	});

	function grow() {
		if (!input) return;
		input.style.height = 'auto';
		input.style.height = `${Math.min(input.scrollHeight, 200)}px`;
	}

	$effect(() => {
		void draft;
		tick().then(grow);
	});

	function where() {
		caret = input?.selectionStart ?? draft.length;
	}

	async function place(text: string, at = text.length) {
		draft = text;
		await tick();
		input?.focus();
		input?.setSelectionRange(at, at);
		caret = at;
	}

	function app(name: 'clear' | 'model' | 'mode', value?: string) {
		sub = null;
		draft = '';
		caret = 0;
		onapp(name, value);
	}

	function command(name: string, kind: 'send' | 'app') {
		if (kind === 'send') {
			place(withCommand(draft, name));
			shut = true;
		} else if (name === 'model' || name === 'mode') {
			sub = name;
		} else {
			app('clear');
		}
	}

	function mention(start: number, path: string) {
		const next = withMention(draft, start, caret, path);
		place(next.text, next.caret);
	}

	function oninput() {
		shut = false;
		sub = null;
		where();
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.isComposing) return;
		const listed = items.length > 0;
		if (listed && (event.key === 'ArrowDown' || event.key === 'ArrowUp')) {
			event.preventDefault();
			active = (active + (event.key === 'ArrowDown' ? 1 : items.length - 1)) % items.length;
			return;
		}
		if (listed && (event.key === 'Enter' || event.key === 'Tab') && !event.shiftKey && !event.metaKey && !event.ctrlKey) {
			event.preventDefault();
			items[active]?.pick();
			return;
		}
		if ((listed || sub) && event.key === 'Escape') {
			// Closes the menu; the work goes on.
			event.preventDefault();
			event.stopPropagation();
			shut = true;
			sub = null;
			return;
		}
		if (event.key === 'Enter' && !event.shiftKey && !event.metaKey && !event.ctrlKey) {
			event.preventDefault();
			onsend();
		}
	}

	/** A picture as PNG, no larger than the model needs. */
	async function asPng(blob: Blob): Promise<string | null> {
		try {
			const bitmap = await createImageBitmap(blob);
			const size = fitted(bitmap.width, bitmap.height);
			const canvas = document.createElement('canvas');
			canvas.width = size.width;
			canvas.height = size.height;
			canvas.getContext('2d')?.drawImage(bitmap, 0, 0, size.width, size.height);
			bitmap.close();
			return canvas.toDataURL('image/png');
		} catch {
			return null;
		}
	}

	async function take(blobs: Blob[]) {
		for (const blob of blobs.filter((b) => b.type.startsWith('image/')).slice(0, 4 - pictures.length)) {
			const png = await asPng(blob);
			if (png) pictures = [...pictures, png];
		}
	}

	function onpaste(event: ClipboardEvent) {
		const blobs = [...(event.clipboardData?.items ?? [])].filter((i) => i.kind === 'file' && i.type.startsWith('image/')).map((i) => i.getAsFile()).filter((f): f is File => f !== null);
		if (blobs.length === 0) return;
		event.preventDefault();
		take(blobs);
	}

	function ondrop(event: DragEvent) {
		dragging = false;
		const blobs = [...(event.dataTransfer?.files ?? [])].filter((f) => f.type.startsWith('image/'));
		if (blobs.length === 0) return;
		event.preventDefault();
		take(blobs);
	}

	function ondragover(event: DragEvent) {
		if ([...(event.dataTransfer?.items ?? [])].some((i) => i.kind === 'file')) {
			event.preventDefault();
			dragging = true;
		}
	}
</script>

<div class="composer" class:off={disabled} class:dragging role="group" aria-label="Message" {ondrop} {ondragover} ondragleave={() => (dragging = false)}>
	{#if items.length}
		<ul class="menu" role="listbox" aria-label={sub ? `Choose the ${sub}` : open?.kind === 'at' ? 'Files' : 'Commands'}>
			{#each items as item, i (item.key)}
				<li role="option" aria-selected={i === active}>
					<button type="button" class:on={i === active} onmousedown={(e) => e.preventDefault()} onclick={() => item.pick()} onmouseenter={() => (active = i)}>
						<span class="label">{item.label}</span>
						{#if item.hint}<span class="hint">{item.hint}</span>{/if}
					</button>
				</li>
			{/each}
		</ul>
	{/if}
	{#if pictures.length}
		<ul class="pictures" aria-label="Pictures">
			{#each pictures as picture, i (picture)}
				<li>
					<img src={picture} alt="Picture {i + 1}" />
					<button type="button" class="drop" onclick={() => (pictures = pictures.filter((_, j) => j !== i))} aria-label="Remove picture {i + 1}">×</button>
				</li>
			{/each}
			{#if !sees}<li class="blind">Not seen by this model</li>{/if}
		</ul>
	{/if}
	<textarea
		bind:this={input}
		bind:value={draft}
		{oninput}
		{onkeydown}
		{onpaste}
		onclick={where}
		onkeyup={where}
		onblur={() => (sub = null)}
		rows="1"
		spellcheck="false"
		{disabled}
		{placeholder}
		aria-label="What should be done"
		aria-autocomplete="list"
	></textarea>
	<div class="tools">{@render tools()}</div>
</div>

<style>
	.composer {
		position: relative;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 18px;
		padding: 4px 6px;
		background: var(--bg);
		box-shadow: 0 1px 2px color-mix(in srgb, var(--ink) 6%, transparent), 0 8px 24px -12px color-mix(in srgb, var(--ink) 18%, transparent);
		transition: border-color var(--quick);
	}

	.composer:focus-within,
	.composer.dragging {
		border-color: var(--accent);
	}

	.composer.off {
		background: var(--well);
	}

	textarea {
		width: 100%;
		min-width: 0;
		max-height: 200px;
		padding: 12px 12px 6px;
		border: 0;
		background: transparent;
		resize: none;
		font: 15px/1.5 var(--rb-text);
		cursor: text;
		scrollbar-width: none;
	}

	textarea:focus-visible {
		outline: none;
	}

	textarea::placeholder {
		color: var(--faint);
	}

	.tools {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 10px;
		min-height: 30px;
		padding: 2px 6px 6px 8px;
	}

	/* The menu opens above the box, over the work. */
	.menu {
		position: absolute;
		left: 0;
		right: 0;
		bottom: calc(100% + 6px);
		z-index: 5;
		max-height: min(320px, 50vh);
		margin: 0;
		padding: 4px;
		overflow: auto;
		list-style: none;
		border: 1px solid var(--line);
		border-radius: 12px;
		background: var(--bg);
		box-shadow: 0 8px 24px -12px color-mix(in srgb, var(--ink) 28%, transparent);
	}

	.menu button {
		display: flex;
		align-items: baseline;
		gap: 12px;
		width: 100%;
		min-height: 30px;
		padding: 5px 10px;
		border-radius: 8px;
		text-align: left;
		font-size: 13.5px;
	}

	.menu button.on {
		background: var(--hover);
	}

	.menu .label {
		flex: none;
		max-width: 70%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 13px var(--rb-data);
		color: var(--ink);
	}

	.menu .hint {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12.5px;
		color: var(--faint);
	}

	.pictures {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		margin: 0;
		padding: 8px 8px 0;
		list-style: none;
	}

	.pictures li {
		position: relative;
	}

	.pictures img {
		display: block;
		width: 56px;
		height: 56px;
		object-fit: cover;
		border-radius: 8px;
		box-shadow: inset 0 0 0 1px var(--line);
	}

	.pictures .drop {
		position: absolute;
		top: -6px;
		right: -6px;
		width: 18px;
		height: 18px;
		border-radius: 50%;
		background: var(--ink);
		color: var(--bg);
		font: 600 12px/18px var(--rb-text);
	}

	.pictures .blind {
		font-size: 12px;
		color: var(--faint);
	}
</style>
