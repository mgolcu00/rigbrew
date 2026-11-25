<script lang="ts">
	import { getAccount } from '#account';
	import { app, type Screen } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { mod } from '#lib/words.ts';
	import Icon from './Icon.svelte';
	import Mark from './Mark.svelte';

	// The one piece of chrome: a new chat, the few places, the chats, and who is using it.
	// Everything else is the conversation. It is also where the window is dragged from.
	const places: { screen: Screen; name: string; icon: 'chat' | 'download' | 'video' }[] = [
		{ screen: 'studio', name: 'Studio', icon: 'video' },
		{ screen: 'models', name: 'Models', icon: 'download' }
	];
	const account = $derived(getAccount());
	const signedIn = $derived(Boolean(account.user && !account.user.anonymous));

	function newChat() {
		chat.newChat();
		app.go('chat');
		chat.focusRequest++;
	}

	function open(id: string) {
		app.go('chat');
		chat.open(id, 'sidebar');
	}
</script>

<aside class="side" aria-label="Rigbrew">
	<div class="top" data-tauri-drag-region>
		<span class="brand" data-tauri-drag-region><Mark size={18} /><span class="wordmark" data-tauri-drag-region>rigbrew</span></span>
	</div>

	<nav class="nav" aria-label="Places">
		<button class="row strong" onclick={newChat} title="New chat ({mod}N)"><Icon name="compose" size={16} /><span>New chat</span></button>
		<button class="row" onclick={() => (app.palette = true)} title="Search chats, models and actions ({mod}K)"><Icon name="search" size={16} /><span>Search</span></button>
		{#each places as p (p.screen)}
			<button class="row" class:current={app.screen === p.screen} aria-current={app.screen === p.screen ? 'page' : undefined} onclick={() => app.go(p.screen)}><Icon name={p.icon} size={16} /><span>{p.name}</span></button>
		{/each}
	</nav>

	<div class="chats scroll">
		{#if chat.conversations.length > 0}
			<p class="head">Chats</p>
			<ul>
				{#each chat.conversations as c (c.id)}
					<li>
						<button class="row chat" class:current={app.screen === 'chat' && chat.activeId === c.id} onclick={() => open(c.id)} title={c.title}><span>{c.title}</span></button>
						<button class="del" onclick={() => chat.remove(c.id)} disabled={chat.busy && chat.activeId === c.id} title="Delete this chat" aria-label="Delete chat “{c.title}”"><Icon name="trash" size={13} /></button>
					</li>
				{/each}
			</ul>
		{/if}
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
</style>
