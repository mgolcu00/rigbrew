<script lang="ts">
	import { tick } from 'svelte';
	import { ux } from '#lib/experience.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { articleById, articles, searchText, type HelpAction } from '#lib/help.ts';
	import { app } from '#lib/state.svelte.ts';
	import { mod } from '#lib/words.ts';
	import Icon from './Icon.svelte';

	// Help: a sheet with a search field, the list of articles and the one that is open. The
	// words live in help.ts. Opened with the "?" in the title bar, Cmd/Ctrl+/ or F1, and by the
	// small "?" marks next to terms, which open their article directly.

	let query = $state('');
	let field = $state<HTMLInputElement>();
	let reader = $state<HTMLElement>();
	let width = $state(800);
	const narrow = $derived(width < 640);

	$effect(() => {
		field?.focus();
	});

	const found = $derived.by(() => {
		const words = query.toLowerCase().split(/\s+/).filter(Boolean);
		if (words.length === 0) return articles;
		return articles.filter((a) => words.every((w) => searchText(a).includes(w)));
	});

	// How many results a search found, never what was searched for; once typing pauses.
	$effect(() => {
		const q = query.trim();
		const results = found.length;
		if (!q) return;
		const timer = setTimeout(() => app.track('help_searched', { results }), 900);
		return () => clearTimeout(timer);
	});

	const article = $derived(articleById(guide.article) ?? (narrow ? null : (found[0] ?? null)));

	async function show(id: string, from: 'list' | 'search' = 'list') {
		guide.showArticle(id, from);
		await tick();
		reader?.scrollTo({ top: 0 });
		reader?.focus();
	}

	function run(does: HelpAction) {
		guide.closeHelp();
		if (does === 'tour') {
			app.go('chat');
			guide.startTour('help');
		} else if (does === 'privacy') app.openPrivacy();
		else if (does === 'models') app.go('models');
		else if (does === 'device') app.go('device');
		else if (does === 'settings') app.openSettings();
		else if (does === 'updates') {
			app.loadConfig();
			app.openSettings();
		}
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && found[0]) {
			event.preventDefault();
			show(found[0].id, 'search');
		}
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="veil" onclick={() => guide.closeHelp()}>
	<div class="sheet" role="dialog" tabindex="-1" aria-modal="true" aria-label="Help" onclick={(e) => e.stopPropagation()} bind:clientWidth={width}>
		<header>
			{#if narrow && article}
				<button class="btn quiet back" onclick={() => (guide.article = '')}><Icon name="back" size={14} /> All topics</button>
			{:else}
				<h1>Help</h1>
			{/if}
			<span class="grow"></span>
			<button class="btn quiet" onclick={() => guide.closeHelp()} aria-label="Close help">Close{#if ux.flags.keyHints}&nbsp;<kbd>esc</kbd>{/if}</button>
		</header>

		<div class="body" class:narrow>
			{#if !narrow || !article}
				<nav class="topics" aria-label="Help topics">
					<label class="search">
						<Icon name="search" size={14} />
						<input bind:this={field} bind:value={query} {onkeydown} placeholder="Search help" aria-label="Search help" spellcheck="false" autocomplete="off" />
					</label>
					<ul class="scroll">
						{#each found as a (a.id)}
							<li><button class:current={article?.id === a.id} aria-current={article?.id === a.id ? 'true' : undefined} onclick={() => show(a.id)}>{a.title}</button></li>
						{:else}
							<li class="none">Nothing matches “{query}”. Try one word, such as “memory” or “picture”.</li>
						{/each}
					</ul>
					<p class="foot">
						<button class="link" onclick={() => run('tour')}>Take the tour again</button>
					</p>
				</nav>
			{/if}

			{#if article}
				<article class="reader scroll selectable" bind:this={reader} tabindex="-1" aria-labelledby="help-title">
					<h2 id="help-title">{article.title}</h2>
					{#each article.body as paragraph, i (i)}
						<p>{paragraph}</p>
					{/each}
					{#if article.terms}
						<dl>
							{#each article.terms as t (t.term)}
								<div><dt>{t.term}</dt><dd>{t.text}</dd></div>
							{/each}
						</dl>
					{/if}
					{#if article.actions}
						<p class="acts">
							{#each article.actions as action (action.does)}
								<button class="btn" onclick={() => run(action.does)}>{action.label}</button>
							{/each}
						</p>
					{/if}
				</article>
			{/if}
		</div>
		{#if !narrow}
			<p class="hint">{ux.flags.keyHints ? `${mod}/ or F1 opens help from anywhere` : 'The ? marks next to words open the right page here'}</p>
		{/if}
	</div>
</div>

<style>
	.veil {
		position: absolute;
		inset: 0;
		z-index: 35;
		display: flex;
		justify-content: center;
		align-items: flex-start;
		padding: 24px 16px 16px;
		background: color-mix(in srgb, var(--rb-board) 62%, transparent);
	}

	.sheet {
		width: min(860px, 100%);
		height: min(620px, 100%);
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 6px;
		background: var(--bg);
		box-shadow: 0 18px 50px -14px var(--shade);
		overflow: hidden;
		animation: appear 0.14s var(--ease) both;
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 8px;
		height: 38px;
		padding: 0 8px 0 14px;
		border-bottom: 1px solid var(--line);
	}

	header h1 {
		font-size: 13px;
	}

	.back {
		margin-left: -8px;
	}

	.grow {
		flex: 1;
	}

	.body {
		flex: 1;
		min-height: 0;
		display: grid;
		grid-template-columns: minmax(220px, 280px) minmax(0, 1fr);
	}

	.body.narrow {
		grid-template-columns: minmax(0, 1fr);
	}

	.topics {
		min-height: 0;
		display: flex;
		flex-direction: column;
		border-right: 1px solid var(--line);
		background: var(--well);
	}

	.narrow .topics {
		border-right: 0;
		background: none;
	}

	.search {
		flex: none;
		display: flex;
		align-items: center;
		gap: 7px;
		height: 30px;
		margin: 10px 10px 6px;
		padding: 0 8px;
		border-radius: var(--r);
		box-shadow: inset 0 0 0 1px var(--line);
		background: var(--bg);
		color: var(--muted);
	}

	.search:focus-within {
		box-shadow: inset 0 0 0 1px var(--accent);
	}

	.search input {
		flex: 1;
		min-width: 0;
		border: 0;
		background: none;
		font-size: 13px;
		color: var(--ink);
		cursor: text;
	}

	.search input:focus-visible {
		outline: none;
	}

	.topics ul {
		flex: 1;
		min-height: 0;
		padding: 0 6px 6px;
	}

	.topics li button {
		display: block;
		width: 100%;
		padding: 5px 8px;
		border-radius: 3px;
		font-size: 13px;
		line-height: 1.35;
		color: var(--muted);
		text-align: left;
	}

	.topics li button:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.topics li button.current {
		background: var(--pressed);
		color: var(--ink);
		font-weight: 500;
	}

	.none {
		padding: 8px;
		font-size: 12.5px;
		color: var(--muted);
	}

	.foot {
		flex: none;
		padding: 8px 14px 10px;
		border-top: 1px solid var(--line-soft);
		font-size: 12.5px;
	}

	.reader {
		min-height: 0;
		padding: 16px 24px 24px;
		font-size: 13.5px;
		line-height: 1.6;
	}

	.reader:focus-visible {
		outline: none;
	}

	.reader h2 {
		margin-bottom: 10px;
		font-size: 16px;
		text-wrap: balance;
	}

	.reader p {
		max-width: 64ch;
	}

	.reader p + p {
		margin-top: 10px;
	}

	dl {
		margin-top: 14px;
		max-width: 68ch;
		border-top: 1px solid var(--line);
	}

	dl div {
		display: grid;
		grid-template-columns: minmax(110px, 30%) minmax(0, 1fr);
		gap: 14px;
		padding: 7px 0;
		border-bottom: 1px solid var(--line-soft);
	}

	dt {
		font-weight: 600;
	}

	dd {
		color: var(--muted);
	}

	.acts {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin-top: 16px;
	}

	.hint {
		flex: none;
		padding: 5px 14px;
		border-top: 1px solid var(--line);
		background: var(--well);
		font-size: 11px;
		color: var(--muted);
	}
</style>
