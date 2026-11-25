<script lang="ts">
	import { app, screens, type Screen } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { isMac, machine, mod } from '#lib/words.ts';
	import Icon from './Icon.svelte';
	import Mark from './Mark.svelte';

	// The window's top row: the mark, the three places, the way into the palette, Help. It is
	// also the drag area. Navigation is small on purpose: chat is what the app is.
	const mac = isMac;
	// Pro keeps the old name for the machine page; the others say what it is.
	const names = $derived<Record<Screen, string>>({ chat: 'Chat', models: 'Models', device: ux.flags.techDetails ? 'Device' : `Your ${machine}`, profile: 'Profile' });
	const tips: Record<Screen, string> = { chat: 'Chat with your models', models: 'Install and manage models', device: 'What this machine can run', profile: 'Your profile' };

	function newChat() {
		chat.newChat();
		app.go('chat');
	}
</script>

<header class="bar" data-tauri-drag-region>
	<div class="brand" data-tauri-drag-region>
		<Mark size={18} />
		<span class="wordmark" aria-label="Rigbrew" data-tauri-drag-region>rig<span>brew</span></span>
	</div>

	<nav aria-label="Places">
		{#each screens as screen, i (screen)}
			<button class:current={app.screen === screen} aria-current={app.screen === screen ? 'page' : undefined} onclick={() => app.go(screen)} title="{tips[screen]}{ux.flags.keyHints ? ` (${mod}${i + 1})` : ''}">
				{names[screen]}{#if ux.flags.tabDigits}<span class="digit num" aria-hidden="true">{i + 1}</span>{/if}
			</button>
		{/each}
	</nav>

	<div class="space" data-tauri-drag-region></div>

	<button class="find" onclick={() => (app.palette = true)} aria-label="Search chats, models and actions" aria-keyshortcuts="{mac ? 'Meta' : 'Control'}+K" title="Search chats, models and actions ({mod}K)">
		<Icon name="search" size={14} />
		<span class="hint">{ux.flags.plainNames ? 'Search' : 'Search chats, models, actions'}</span>
		{#if ux.flags.keyHints}<kbd>{mod}K</kbd>{/if}
	</button>

	<div class="tools">
		<button class="tool" onclick={newChat} title="New chat{ux.flags.keyHints ? ` (${mod}N)` : ''}" aria-label="New chat"><Icon name="compose" size={16} /></button>
		{#if app.screen === 'chat'}
			<button class="tool wide" class:on={app.pane} onclick={() => app.togglePane()} title="{app.pane ? 'Hide' : 'Show'} the list of chats{ux.flags.keyHints ? ` (${mod}\\)` : ''}" aria-label="{app.pane ? 'Hide' : 'Show'} the list of chats" aria-pressed={app.pane}><Icon name="pane" size={16} /></button>
		{/if}
		{#if app.sponsors}
			<button class="tool heart" onclick={() => app.openSponsor('titlebar')} title="Sponsor Rigbrew on GitHub. It is free; sponsors pay for the builds." aria-label="Sponsor Rigbrew on GitHub"><Icon name="heart" size={16} /></button>
		{/if}
		<button class="tool" class:on={app.sheet !== null} onclick={() => app.openSettings()} title="Settings{ux.flags.keyHints ? ` (${mod},)` : ''}" aria-label="Settings"><Icon name="gear" size={16} /></button>
		<button
			class="tool help"
			class:labelled={ux.flags.helpLabel}
			class:on={guide.helpOpen}
			data-tour="help"
			onclick={() => (guide.helpOpen ? guide.closeHelp() : guide.openHelp('', 'titlebar'))}
			title="Help{ux.flags.keyHints ? ` (${mod}/ or F1)` : ''}"
			aria-label="Help"
			aria-keyshortcuts="F1 {mac ? 'Meta' : 'Control'}+/"
		>
			<span class="qmark" aria-hidden="true">?</span>{#if ux.flags.helpLabel}<span class="word">Help</span>{/if}
		</button>
	</div>
</header>

<style>
	.bar {
		flex: none;
		display: flex;
		align-items: stretch;
		gap: 14px;
		height: var(--titlebar);
		padding: 0 10px 0 14px;
		border-bottom: 1px solid var(--line);
		background: var(--board);
	}

	/* Leave room for the traffic lights. */
	:global(html[data-native][data-platform='mac']) .bar {
		padding-left: 82px;
	}

	.brand {
		display: flex;
		align-items: center;
		gap: 7px;
	}

	.wordmark {
		font: 600 13px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.wordmark span {
		font-weight: 300;
	}

	nav {
		display: flex;
		align-items: stretch;
		margin-left: 6px;
	}

	nav button {
		position: relative;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 11px;
		color: var(--muted);
		font-weight: 500;
		transition: color var(--quick);
	}

	nav button:hover {
		color: var(--ink);
	}

	nav button.current {
		color: var(--ink);
	}

	/* The current place: a rule that sits on the title bar's own hairline. */
	nav button.current::after {
		content: '';
		position: absolute;
		inset: auto 8px -1px;
		height: 2px;
		background: var(--ink);
	}

	nav button:focus-visible {
		outline-offset: -3px;
	}

	.digit {
		font-size: 10.5px;
		color: var(--muted);
	}

	.space {
		flex: 1;
		min-width: 8px;
	}

	.find {
		align-self: center;
		display: flex;
		align-items: center;
		gap: 7px;
		width: clamp(190px, 26vw, 300px);
		height: 26px;
		padding: 0 6px 0 8px;
		border-radius: var(--r);
		box-shadow: inset 0 0 0 1px var(--line);
		background: var(--bg);
		color: var(--muted);
		transition: box-shadow var(--quick);
	}

	.find:hover {
		box-shadow: inset 0 0 0 1px var(--muted);
	}

	.find .hint {
		flex: 1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12.5px;
	}

	.tools {
		display: flex;
		align-items: center;
		gap: 2px;
	}

	.tool {
		display: grid;
		place-items: center;
		width: 28px;
		height: 26px;
		border-radius: var(--r);
		color: var(--muted);
		transition:
			background-color var(--quick),
			color var(--quick);
	}

	.tool:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.tool.heart:hover {
		color: var(--accent);
	}

	.tool.on {
		color: var(--ink);
		background: var(--pressed);
	}

	.help {
		display: flex;
		gap: 6px;
		width: auto;
		min-width: 28px;
		padding: 0 6px;
	}

	.qmark {
		display: grid;
		place-items: center;
		width: 16px;
		height: 16px;
		border-radius: 50%;
		box-shadow: inset 0 0 0 1.5px currentColor;
		font: 700 10.5px/1 var(--rb-text);
	}

	/* Guided: Help is a labelled button, the one thing a newcomer should be able to find. */
	.help.labelled {
		margin-left: 4px;
		padding: 0 9px 0 7px;
		box-shadow: inset 0 0 0 1px var(--line);
		background: var(--bg);
		color: var(--ink);
		font-weight: 500;
	}

	.help.labelled:hover {
		box-shadow: inset 0 0 0 1px var(--accent);
		color: var(--accent);
		background: var(--bg);
	}

	@media (max-width: 880px) {
		.wordmark,
		.find .hint {
			display: none;
		}

		.find {
			width: auto;
		}

		.bar {
			gap: 8px;
		}
	}

	@media (max-width: 1079px) {
		.tool.wide {
			display: none;
		}
	}
</style>
