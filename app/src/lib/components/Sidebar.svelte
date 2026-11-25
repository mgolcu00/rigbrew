<script lang="ts">
	import { onMount } from 'svelte';
	import { getAccount } from '#account';
	import { app, type Screen } from '#lib/state.svelte.ts';
	import { agents } from '#lib/agents.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { code } from '#lib/code.svelte.ts';
	import { connectors } from '#lib/connectors.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { codeRows, historyTabs, tabFrom, tabOf, type CodeRow, type HistoryTab } from '#lib/history.ts';
	import { remembered } from '#lib/remember.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { skills } from '#lib/skills.svelte.ts';
	import { mod } from '#lib/words.ts';
	import AgentsSheet from './AgentsSheet.svelte';
	import ConnectorsSheet from './ConnectorsSheet.svelte';
	import Icon from './Icon.svelte';
	import Mark from './Mark.svelte';
	import SkillsSheet from './SkillsSheet.svelte';

	// The one piece of chrome: a new chat, the few places, the history, and who is using it.
	// Everything else is the conversation. It is also where the window is dragged from.
	const places: { screen: Screen; name: string; icon: 'chat' | 'download' | 'video' | 'code' }[] = [
		{ screen: 'code', name: 'Code', icon: 'code' },
		{ screen: 'studio', name: 'Studio', icon: 'video' },
		{ screen: 'models', name: 'Models', icon: 'download' }
	];
	const account = $derived(getAccount());
	const signedIn = $derived(Boolean(account.user && !account.user.anonymous));

	// A task may be working already when the window opens (it runs in the service): follow it.
	onMount(() => void code.wake());
	// What the chat is told about skills and connectors comes from these two lists.
	onMount(() => {
		void skills.load();
		void connectors.load();
	});

	// The three kinds of tools sit behind one row; it remembers whether it was left open.
	let tools = $state(false);
	onMount(() => {
		try {
			tools = localStorage.getItem('rigbrew.sidebar.tools') === '1';
		} catch {
			// no storage: closed
		}
	});
	function toggleTools() {
		tools = !tools;
		try {
			localStorage.setItem('rigbrew.sidebar.tools', tools ? '1' : '0');
		} catch {
			// not remembered
		}
	}

	function newChat() {
		chat.newChat();
		app.go('chat');
		chat.focusRequest++;
	}

	function open(id: string) {
		app.go('chat');
		chat.open(id, 'sidebar');
	}

	// The history: chats, the Code tasks of every folder, and films, one list at a time. The tab
	// follows the screen, and the one chosen last is remembered.
	onMount(() => void studio.refresh());
	const tasks = $derived(codeRows(code.all));
	const films = $derived(studio.films.filter((f) => !f.missing));
	const shown = $derived(historyTabs.filter((t) => t.id === 'chat' || (t.id === 'code' ? tasks.length > 0 : films.length > 0)));
	let chosen = $state<string | null>(remembered.get('sidebar.history'));
	const tab = $derived(tabFrom(chosen, shown.map((t) => t.id)));
	function choose(id: HistoryTab) {
		chosen = id;
		remembered.set('sidebar.history', id);
	}
	let screenBefore = app.screen;
	$effect(() => {
		const screen = app.screen;
		if (screen === screenBefore) return;
		screenBefore = screen;
		const own = tabOf(screen);
		if (own) choose(own);
	});

	/** Left and right move between the tabs, as in any row of tabs. */
	function tabKey(event: KeyboardEvent) {
		const step = event.key === 'ArrowRight' ? 1 : event.key === 'ArrowLeft' ? -1 : 0;
		if (!step) return;
		event.preventDefault();
		const at = shown.findIndex((t) => t.id === tab);
		const next = shown[(at + step + shown.length) % shown.length];
		choose(next.id);
		(event.currentTarget as HTMLElement).querySelector<HTMLButtonElement>(`[data-tab="${next.id}"]`)?.focus();
	}

	function openTask(row: CodeRow) {
		app.go('code');
		void code.show(row);
	}

	function openFilm(id: string) {
		app.go('studio');
		if (studio.id !== id) void studio.open(id);
	}
</script>

<aside class="side" aria-label="Rigbrew">
	<div class="top" data-tauri-drag-region>
		<span class="brand" data-tauri-drag-region><Mark size={18} /><span class="wordmark" data-tauri-drag-region>rigbrew</span></span>
	</div>

	<nav class="nav" aria-label="Places">
		<button class="row strong" onclick={newChat} title="New chat ({mod}N)"><Icon name="compose" size={16} /><span>New chat</span></button>
		<button class="row" onclick={() => (app.palette = true)} title="Search chats, tasks, models and actions ({mod}K)"><Icon name="search" size={16} /><span>Search</span></button>
		{#each places as p (p.screen)}
			<button class="row" class:current={app.screen === p.screen} aria-current={app.screen === p.screen ? 'page' : undefined} onclick={() => app.go(p.screen)}>
				<Icon name={p.icon} size={16} /><span>{p.name}</span>
				{#if p.screen === 'code' && code.waiting > 0}
					<span class="badge" title="A task waits for your answer">{code.waiting}</span>
				{:else if p.screen === 'code' && code.working}
					<span class="busy" title="A task is working"></span>
				{/if}
			</button>
		{/each}
		<!-- Agents, skills and connectors: one quiet row that opens to three, so the list stays short. -->
		<button class="row" aria-expanded={tools} onclick={toggleTools} title="Agents, skills and connectors">
			<Icon name="library" size={16} /><span>Tools</span>
			{#if connectors.usable.length > 0 && !tools}<span class="count" title="{connectors.usable.length} connectors running">{connectors.usable.length}</span>{/if}
			<span class="chev" class:open={tools} aria-hidden="true"><Icon name="chevron" size={12} /></span>
		</button>
		{#if tools}
			<button class="row sub" class:current={agents.sheet !== null} onclick={() => agents.open('sidebar')} title="Agents: saved ways to use the chat model"><span>Agents</span></button>
			<button class="row sub" class:current={skills.sheet !== null} onclick={() => skills.open('sidebar', code.folder)} title="Skills: instructions for one kind of work"><span>Skills</span></button>
			<button class="row sub" class:current={connectors.sheet !== null} onclick={() => connectors.open('sidebar')} title="Connectors: tools from outside Rigbrew">
				<span>Connectors</span>
				{#if connectors.usable.length > 0}<span class="count" title="{connectors.usable.length} running">{connectors.usable.length}</span>{/if}
			</button>
		{/if}
	</nav>

	<div class="chats scroll">
		{#if shown.length > 1}
			<!-- The tabs take the focus, not the row they are in. -->
			<!-- svelte-ignore a11y_interactive_supports_focus -->
			<div class="tabs" role="tablist" aria-label="History" onkeydown={tabKey}>
				{#each shown as t (t.id)}
					<button role="tab" id="history-{t.id}" data-tab={t.id} aria-selected={tab === t.id} aria-controls="history-list" tabindex={tab === t.id ? 0 : -1} onclick={() => choose(t.id)}>{t.name}</button>
				{/each}
			</div>
		{:else if chat.conversations.length > 0}
			<p class="head">Chats</p>
		{/if}
		<ul id="history-list" aria-labelledby={shown.length > 1 ? `history-${tab}` : undefined}>
			{#if tab === 'chat'}
				{#each chat.conversations as c (c.id)}
					<li>
						<button class="row chat" class:current={app.screen === 'chat' && chat.activeId === c.id} onclick={() => open(c.id)} title={c.title}><span>{c.title}</span></button>
						<button class="del" onclick={() => chat.remove(c.id)} disabled={chat.busy && chat.activeId === c.id} title="Delete this chat" aria-label="Delete chat “{c.title}”"><Icon name="trash" size={13} /></button>
					</li>
				{/each}
			{:else if tab === 'code'}
				{#each tasks as t (t.id)}
					<li>
						<button class="row chat" class:current={app.screen === 'code' && code.task?.id === t.id} onclick={() => openTask(t)} title="{t.title} · {t.folder}">
							<span class="name">{t.title}</span>
							<span class="where">{t.where}</span>
							{#if t.mark === 'waiting'}
								<span class="mark" title="Waits for your answer" role="img" aria-label="Waits for your answer"></span>
							{:else if t.mark === 'working'}
								<span class="mark live" title="Working" role="img" aria-label="Working"></span>
							{/if}
						</button>
						<!-- A task that is working is stopped first, on its own screen. -->
						{#if !t.mark}
							<button class="del" onclick={() => code.remove(t.id)} title="Remove this task from the list" aria-label="Remove task “{t.title}”"><Icon name="trash" size={13} /></button>
						{/if}
					</li>
				{/each}
			{:else}
				{#each films as f (f.id)}
					<li>
						<button class="row chat" class:current={app.screen === 'studio' && studio.id === f.id} onclick={() => openFilm(f.id)} title="{f.name} · {f.folder}"><span>{f.name}</span></button>
					</li>
				{/each}
			{/if}
		</ul>
	</div>

	<div class="foot">
		<button class="row who" class:current={app.screen === 'profile'} onclick={() => app.go('profile')} title="Your profile and account">
			<span class="avatar" aria-hidden="true">{(account.label || 'Y').slice(0, 1).toUpperCase()}</span>
			<span class="name">{signedIn ? account.label : 'Sign in'}</span>
		</button>
		{#if app.sponsors}
			<button class="ico" onclick={() => app.openSponsor('sidebar')} title="Sponsor Rigbrew on GitHub" aria-label="Sponsor Rigbrew on GitHub"><Icon name="heart" size={16} /></button>
		{/if}
		<button class="ico" onclick={() => guide.openHelp('', 'sidebar')} title="Help" aria-label="Help">?</button>
		<button class="ico" onclick={() => app.openSettings()} title="Settings ({mod},)" aria-label="Settings"><Icon name="gear" size={16} /></button>
	</div>
</aside>

<AgentsSheet />
<SkillsSheet />
<ConnectorsSheet />

<style>
	.side {
		flex: none;
		width: 248px;
		display: flex;
		flex-direction: column;
		min-height: 0;
		background: var(--side);
		border-right: 1px solid var(--line-soft);
		font-family: var(--rb-text);
	}

	/* Room for the window buttons on the Mac. */
	.top {
		flex: none;
		height: 52px;
		display: flex;
		align-items: flex-end;
		padding: 0 14px 8px;
	}

	:global([data-native][data-platform='mac']) .top {
		justify-content: flex-end;
	}

	.brand {
		display: inline-flex;
		align-items: center;
		gap: 8px;
	}

	.wordmark {
		font: 600 14px/1 var(--rb-text);
		letter-spacing: -0.01em;
	}

	.nav {
		flex: none;
		display: grid;
		gap: 1px;
		padding: 6px 8px 10px;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		min-width: 0;
		height: 32px;
		padding: 0 10px;
		border-radius: 8px;
		font-size: 14px;
		color: var(--ink);
		text-align: left;
	}

	.row span {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.row:hover {
		background: var(--hover);
	}

	.row.current {
		background: var(--pressed);
	}

	.row.strong {
		font-weight: 600;
	}

	/* Code: a task waits for an answer, or is working in the background. */
	.row .badge {
		margin-left: auto;
		min-width: 18px;
		height: 18px;
		padding: 0 5px;
		border-radius: 9px;
		background: var(--accent);
		color: var(--on-accent);
		font: 600 11px/18px var(--rb-text);
		text-align: center;
		overflow: visible;
	}

	.row .count {
		margin-left: auto;
		overflow: visible;
		font: 500 11px/1 var(--rb-data);
		color: var(--faint);
	}

	.row .busy {
		margin-left: auto;
		flex: none;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--accent);
		animation: blink 1.2s steps(2, jump-none) infinite;
	}

	.chats {
		flex: 1;
		min-height: 0;
		padding: 0 8px 8px;
	}

	.head {
		padding: 8px 10px 6px;
		font-size: 12px;
		color: var(--faint);
	}

	/* Which history is shown: a quiet row of two or three words. */
	.tabs {
		display: flex;
		gap: 2px;
		margin: 4px 2px 6px;
		padding: 2px;
		border-radius: 8px;
		background: var(--hover);
	}

	.tabs button {
		flex: 1;
		min-width: 0;
		height: 24px;
		border-radius: 6px;
		font-size: 12px;
		color: var(--muted);
	}

	.tabs button:hover {
		color: var(--ink);
	}

	.tabs button[aria-selected='true'] {
		background: var(--side);
		box-shadow: 0 0 0 1px var(--line-soft);
		color: var(--ink);
		font-weight: 600;
	}

	ul {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 1px;
	}

	li {
		position: relative;
	}

	.chat {
		padding-right: 30px;
		color: var(--muted);
	}

	.chat.current {
		color: var(--ink);
	}

	/* A Code task: its name, the folder it works in, and a dot while it works or waits. */
	.chat .name {
		flex: 1;
	}

	.chat .where {
		flex: none;
		max-width: 38%;
		font-size: 11.5px;
		color: var(--faint);
	}

	.chat .mark {
		flex: none;
		width: 6px;
		height: 6px;
		margin-right: -18px;
		border-radius: 50%;
		background: var(--accent);
		overflow: visible;
	}

	.chat .mark.live {
		animation: blink 1.2s steps(2, jump-none) infinite;
	}

	.del {
		position: absolute;
		right: 4px;
		top: 4px;
		width: 24px;
		height: 24px;
		display: none;
		place-items: center;
		border-radius: 6px;
		color: var(--muted);
	}

	li:hover .del,
	.del:focus-visible {
		display: grid;
	}

	.del:hover {
		background: var(--pressed);
		color: var(--fail);
	}

	.foot {
		flex: none;
		display: flex;
		align-items: center;
		gap: 2px;
		padding: 8px;
		border-top: 1px solid var(--line-soft);
	}

	.who {
		flex: 1;
		height: 36px;
	}

	.avatar {
		flex: none;
		width: 24px;
		height: 24px;
		display: grid;
		place-items: center;
		border-radius: 50%;
		background: var(--accent);
		color: var(--on-accent);
		font-size: 12px;
		font-weight: 600;
	}

	.ico {
		flex: none;
		width: 30px;
		height: 30px;
		display: grid;
		place-items: center;
		border-radius: 8px;
		color: var(--muted);
		font-weight: 600;
	}

	.ico:hover {
		background: var(--hover);
		color: var(--ink);
	}

	@media (max-width: 760px) {
		.side {
			width: 200px;
		}
	}

	.row.sub {
		height: 30px;
		padding-left: 36px;
		font-size: 13.5px;
		color: var(--muted);
	}

	.chev {
		margin-left: auto;
		display: inline-grid;
		color: var(--faint);
		transition: transform var(--quick);
	}

	.chev.open {
		transform: rotate(90deg);
	}

	.row .count + .chev {
		margin-left: 6px;
	}
</style>
