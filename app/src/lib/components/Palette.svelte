<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { app, type Theme } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { LEVELS, LEVEL_ORDER, TEXT_SIZES, ux } from '#lib/experience.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { articles } from '#lib/help.ts';
	import { ago, familyOf, installedVariant, size } from '#lib/format.ts';
	import { machine, mod, nameOf, roleOf, versionLabel } from '#lib/words.ts';
	import Icon, { type IconName } from './Icon.svelte';

	// The command palette: one field that finds chats, places, models, actions and help.
	// Everything the app can do is reachable from here without the pointer.

	onMount(() => {
		guide.count('palette');
		app.track('palette_opened');
	});

	/** What was run, as a label without names or text: "new", "chat", "use_model"… */
	function actionOf(id: string): { action: string; package?: string; article?: string } {
		const m = id.match(/^(use|install|show)-(.+)$/);
		if (m) return { action: `${m[1]}_model`, package: m[2] };
		if (id.startsWith('chat-')) return { action: 'open_chat' };
		if (id.startsWith('help-')) return { action: 'help_article', article: id.slice(5) };
		if (id.startsWith('level-')) return { action: 'level' };
		if (id.startsWith('text-')) return { action: 'text_size' };
		if (id.startsWith('theme-')) return { action: 'theme' };
		return { action: id.replaceAll('-', '_') };
	}

	const flags = $derived(ux.flags);

	type Group = 'Actions' | 'Go to' | 'Chats' | 'Models' | 'Settings' | 'Help';
	const order: Group[] = ['Actions', 'Chats', 'Go to', 'Models', 'Settings', 'Help'];

	interface Item {
		id: string;
		group: Group;
		title: string;
		detail?: string;
		keys?: string;
		icon: IconName;
		/** Extra words the search also looks at. */
		words?: string;
		run: () => void;
	}

	let query = $state('');
	let cursor = $state(0);
	let list = $state<HTMLElement>();
	let field = $state<HTMLInputElement>();

	$effect(() => {
		field?.focus();
	});

	const close = () => (app.palette = false);

	function toChat() {
		app.go('chat');
		chat.focusRequest++;
	}

	const items = $derived.by((): Item[] => {
		const out: Item[] = [];
		const hasChat = app.chatPackages.length > 0;
		const loaded = app.packages.find((p) => p.id === chat.engine?.package);
		const keys = (k: string) => (flags.keyHints ? k : undefined);

		out.push({
			id: 'new',
			group: 'Actions',
			title: 'New chat',
			keys: keys(`${mod}N`),
			icon: 'compose',
			run: () => {
				chat.newChat();
				app.go('chat');
			}
		});
		if (app.imagePackage || app.imageOffer) {
			out.push(
				chat.mode === 'image' && hasChat
					? { id: 'mode', group: 'Actions', title: 'Switch to Chat', detail: 'Answers in words', keys: keys(`${mod}⇧I`), icon: 'chat', words: 'text mode switch', run: () => (chat.setMode('chat', 'shortcut'), toChat()) }
					: { id: 'mode', group: 'Actions', title: 'Make a picture', detail: `Picture mode: descriptions go to ${app.imagePackage ? nameOf(app.imagePackage, flags) : 'the picture model'}`, keys: keys(`${mod}⇧I`), icon: 'image', words: 'image picture draw switch mode', run: () => (chat.setMode('image', 'shortcut'), toChat()) }
			);
		}
		const open = chat.active;
		if (open) {
			out.push({ id: 'rename', group: 'Actions', title: 'Rename this chat', detail: open.title, icon: 'compose', words: 'title name', run: () => renameActive() });
			out.push({ id: 'delete', group: 'Actions', title: 'Delete this chat', detail: 'Undo is offered for a few seconds', icon: 'trash', words: 'remove', run: () => chat.remove(open.id) });
		}
		if (loaded) {
			out.push({
				id: 'unload',
				group: 'Actions',
				title: 'Unload the model',
				detail: `${nameOf(loaded, flags)} leaves memory`,
				keys: keys(`${mod}⇧U`),
				icon: 'eject',
				words: 'free memory eject',
				run: () => {
					app.track('unload_clicked', { from: 'palette' });
					chat
						.unload()
						.then(() => app.say(`Unloaded ${nameOf(loaded, flags)}.`))
						.catch(() => chat.syncEngine());
				}
			});
		}
		if (chat.busy) out.push({ id: 'stop', group: 'Actions', title: 'Stop the answer', keys: 'Esc', icon: 'stop', run: () => chat.stop() });
		out.push({ id: 'pane', group: 'Actions', title: app.pane ? 'Hide the list of chats' : 'Show the list of chats', keys: keys(`${mod}\\`), icon: 'pane', words: 'sidebar side pane panel chats', run: () => (app.togglePane(), app.go('chat')) });
		out.push({ id: 'updates', group: 'Actions', title: 'Check for updates', icon: 'redo', run: () => (app.loadConfig(), app.openSettings()) });
		if ((app.usage?.sync.pending ?? 0) > 0) {
			out.push({
				id: 'sync',
				group: 'Actions',
				title: 'Send usage data now',
				detail: `${app.usage!.sync.pending} waiting`,
				icon: 'sync',
				run: () => {
					app.sync().then((problem) => app.say(problem ? `Not sent: ${problem}.` : 'Usage data sent.'));
				}
			});
		}
		if (app.sponsors) out.push({ id: 'sponsor', group: 'Actions', title: 'Support Rigbrew', icon: 'heart', words: 'sponsor donate', run: () => app.openSponsor('palette') });

		out.push({ id: 'go-chat', group: 'Go to', title: 'Chat', keys: keys(`${mod}1`), icon: 'chat', run: toChat });
		out.push({ id: 'go-code', group: 'Go to', title: 'Code', icon: 'code', words: 'agent folder project files repo task coding', run: () => app.go('code') });
		out.push({ id: 'go-models', group: 'Go to', title: 'Models', keys: keys(`${mod}2`), icon: 'library', words: 'library install builds versions', run: () => app.go('models') });
		out.push({ id: 'go-device', group: 'Go to', title: flags.techDetails ? 'Device' : `Your ${machine}`, keys: keys(`${mod}3`), icon: 'rig', words: 'device rig hardware machine memory mac computer', run: () => app.go('device') });
		out.push({ id: 'go-profile', group: 'Go to', title: 'Profile', icon: 'person', words: 'account activity usage sign in streak', run: () => app.go('profile') });
		out.push({ id: 'go-settings', group: 'Go to', title: 'Settings', keys: keys(`${mod},`), icon: 'gear', words: 'preferences appearance updates storage', run: () => app.openSettings() });
		out.push({ id: 'go-privacy', group: 'Go to', title: 'Privacy and data', icon: 'privacy', words: 'consent queue events withdraw', run: () => app.openPrivacy() });

		for (const c of chat.conversations) {
			out.push({
				id: `chat-${c.id}`,
				group: 'Chats',
				title: c.title,
				detail: c.loaded ? `${c.messages.filter((m) => m.role === 'user').length} sent · ${ago(c.updatedAt)}` : ago(c.updatedAt),
				icon: 'chat',
				run: () => {
					chat.open(c.id, 'palette');
					app.go('chat');
				}
			});
		}

		for (const pkg of app.packages) {
			const family = familyOf(pkg);
			const build = installedVariant(pkg);
			const rec = pkg.recommendation;
			const version = build ? versionLabel(pkg, build.id, flags) : '';
			const facts = (b: { accel_mem_mb: number }) => [version, `${size(b.accel_mem_mb)} memory`].filter(Boolean).join(' · ');
			if (build && pkg.modality === 'chat') {
				out.push({
					id: `use-${pkg.id}`,
					group: 'Models',
					title: `Chat with ${nameOf(pkg, flags)}`,
					detail: `${facts(build)}${chat.model === pkg.id ? ' · in use' : ''}`,
					icon: 'chat',
					run: () => {
						chat.model = pkg.id;
						chat.setMode('chat', 'click');
						toChat();
					}
				});
			} else if (build && pkg.modality === 'image') {
				out.push({ id: `use-${pkg.id}`, group: 'Models', title: `Make a picture with ${nameOf(pkg, flags)}`, detail: facts(build), icon: 'image', run: () => (chat.setMode('image', 'click'), toChat()) });
			} else if (build && pkg.modality === 'video') {
				out.push({ id: `use-${pkg.id}`, group: 'Models', title: `Make a clip with ${nameOf(pkg, flags)}`, detail: facts(build), icon: 'video', run: () => (chat.setMode('video', 'click'), toChat()) });
			} else if (!build && rec.verdict === 'run' && !app.installs[pkg.id]) {
				out.push({
					id: `install-${pkg.id}`,
					group: 'Models',
					title: `Install ${nameOf(pkg, flags)}`,
					detail: `${roleOf(pkg)} · ${size(rec.disk_mb)} download`,
					icon: 'download',
					run: () => {
						app.track('install_clicked', { package: pkg.id, variant: rec.variant, from: 'palette' });
						app.install(pkg);
						app.showModel(pkg.id);
					}
				});
			}
			out.push({
				id: `show-${pkg.id}`,
				group: 'Models',
				title: `${nameOf(pkg, flags)} in Models`,
				detail: flags.buildsTable ? `${family.does} · ${pkg.variants.length} ${pkg.variants.length === 1 ? 'build' : 'builds'}` : roleOf(pkg),
				icon: family.icon,
				words: `${family.name} ${family.does} ${pkg.name}`,
				run: () => app.showModel(pkg.id)
			});
		}

		// Settings that people look for by name: the level, text size, appearance.
		for (const level of LEVEL_ORDER) {
			if (level === ux.level) continue;
			out.push({ id: `level-${level}`, group: 'Settings', title: `Switch to ${LEVELS[level].name}`, detail: LEVELS[level].line, icon: 'settings', words: 'experience level guided standard pro simple advanced more less beginner expert', run: () => app.setLevel(level, 'palette') });
		}
		for (const t of TEXT_SIZES) {
			if (t.id === ux.textSize) continue;
			out.push({ id: `text-${t.id}`, group: 'Settings', title: `Text size: ${t.name}`, icon: 'settings', words: 'font zoom bigger smaller larger accessibility', run: () => (app.setTextSize(t.id, 'palette'), app.say(`Text size: ${t.name}.`)) });
		}
		for (const theme of ['system', 'light', 'dark'] as Theme[]) {
			if (theme === app.theme) continue;
			out.push({ id: `theme-${theme}`, group: 'Settings', title: `Appearance: ${theme}`, icon: 'system', words: 'theme dark light mode colour color', run: () => app.setTheme(theme) });
		}

		out.push({ id: 'help', group: 'Help', title: 'Open help', keys: keys(`${mod}/`), icon: 'search', words: 'help manual how', run: () => guide.openHelp('', 'palette') });
		for (const a of articles) {
			out.push({ id: `help-${a.id}`, group: 'Help', title: a.title, icon: 'search', words: 'help', run: () => guide.openHelp(a.id, 'palette') });
		}
		return out;
	});

	/** Rename from the palette: the chat's own name field takes over. */
	function renameActive() {
		app.go('chat');
		tick().then(() => document.querySelector<HTMLButtonElement>('.chat .head .title')?.click());
	}

	/** Subsequence match. Letters in a row and at the start of a word count for more. */
	function match(needle: string, text: string): { score: number; hits: Set<number> } | null {
		const t = text.toLowerCase();
		const hits = new Set<number>();
		let at = 0;
		let previous = -2;
		let score = 0;
		for (const ch of needle) {
			if (ch === ' ') continue;
			const found = t.indexOf(ch, at);
			if (found < 0) return null;
			score += 1;
			if (found === previous + 1) score += 3;
			if (found === 0 || /[\s\-·/:]/.test(t[found - 1])) score += 4;
			score -= (found - at) * 0.08;
			hits.add(found);
			previous = found;
			at = found + 1;
		}
		return { score: score - t.length * 0.01, hits };
	}

	interface Row {
		item: Item;
		score: number;
		hits: Set<number>;
	}

	const groups = $derived.by(() => {
		const needle = query.trim().toLowerCase();
		const rows: Row[] = [];
		for (const item of items) {
			if (!needle) {
				rows.push({ item, score: 0, hits: new Set() });
				continue;
			}
			const title = match(needle, item.title);
			const other = title ? null : match(needle, `${item.words ?? ''} ${item.detail ?? ''}`);
			if (title) rows.push({ item, score: title.score + 2, hits: title.hits });
			else if (other) rows.push({ item, score: other.score * 0.6, hits: new Set() });
		}
		const out = order
			.map((group) => {
				let own = rows.filter((r) => r.item.group === group);
				if (needle) own.sort((a, b) => b.score - a.score);
				else if (group === 'Chats') own = own.slice(0, 6);
				else if (group === 'Help') own = own.slice(0, 2);
				else if (group === 'Settings') own = own.filter((r) => r.item.id.startsWith('level-'));
				return { group, rows: own, best: Math.max(-Infinity, ...own.map((r) => r.score)) };
			})
			.filter((g) => g.rows.length > 0);
		if (needle) out.sort((a, b) => b.best - a.best);
		return out;
	});

	const flat = $derived(groups.flatMap((g) => g.rows));

	$effect(() => {
		query;
		cursor = 0;
	});

	async function move(by: number) {
		if (flat.length === 0) return;
		cursor = (cursor + by + flat.length) % flat.length;
		await tick();
		list?.querySelector('[aria-selected="true"]')?.scrollIntoView({ block: 'nearest' });
	}

	function run(row: Row | undefined) {
		if (!row) return;
		app.track('palette_action', { ...actionOf(row.item.id), searched: query.trim().length > 0 });
		close();
		row.item.run();
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'ArrowDown' || (event.ctrlKey && event.key === 'n')) {
			event.preventDefault();
			move(1);
		} else if (event.key === 'ArrowUp' || (event.ctrlKey && event.key === 'p')) {
			event.preventDefault();
			move(-1);
		} else if (event.key === 'Enter' && !event.isComposing) {
			event.preventDefault();
			run(flat[cursor]);
		} else if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			close();
		}
	}

	const letters = (text: string) => [...text];
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="veil" onclick={close}>
	<div class="palette" role="dialog" tabindex="-1" aria-label="Search and commands" onclick={(e) => e.stopPropagation()}>
		<div class="field">
			<Icon name="search" size={15} />
			<input
				bind:this={field}
				bind:value={query}
				{onkeydown}
				spellcheck="false"
				autocomplete="off"
				placeholder="Search chats, models, settings and help"
				role="combobox"
				aria-expanded="true"
				aria-controls="palette-list"
				aria-activedescendant={flat[cursor] ? `palette-${flat[cursor].item.id}` : undefined}
			/>
			<kbd>esc</kbd>
		</div>

		<div class="list scroll" id="palette-list" role="listbox" bind:this={list}>
			{#each groups as group (group.group)}
				<p class="label group">{group.group}</p>
				{#each group.rows as row (row.item.id)}
					{@const selected = flat[cursor] === row}
					<!-- svelte-ignore a11y_no_noninteractive_tabindex -->
					<div class="row" id="palette-{row.item.id}" role="option" tabindex="-1" aria-selected={selected} onclick={() => run(row)} onmousemove={() => (cursor = flat.indexOf(row))}>
						<Icon name={row.item.icon} size={15} />
						<span class="title">{#each letters(row.item.title) as letter, i (i)}{#if row.hits.has(i)}<b>{letter}</b>{:else}{letter}{/if}{/each}</span>
						{#if row.item.detail}<span class="detail num">{row.item.detail}</span>{/if}
						{#if row.item.keys}<kbd>{row.item.keys}</kbd>{/if}
					</div>
				{/each}
			{:else}
				<p class="none">Nothing matches “{query}”.</p>
			{/each}
		</div>

		<p class="foot num">
			<span><kbd>↑</kbd><kbd>↓</kbd> move</span>
			<span><kbd>↵</kbd> run</span>
			<span class="count">{flat.length} {flat.length === 1 ? 'result' : 'results'}</span>
		</p>
	</div>
</div>

<style>
	.veil {
		position: fixed;
		inset: 0;
		z-index: 60;
		display: flex;
		justify-content: center;
		align-items: flex-start;
		padding: min(14vh, 120px) 16px 16px;
		background: color-mix(in srgb, var(--rb-board) 55%, transparent);
	}

	.palette {
		width: min(620px, 100%);
		max-height: min(520px, 100%);
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 6px;
		background: var(--bg);
		box-shadow:
			0 0 0 1px color-mix(in srgb, var(--ink) 4%, transparent),
			0 18px 50px -12px var(--shade);
		overflow: hidden;
		animation: appear 0.12s var(--ease) both;
	}

	.field {
		flex: none;
		display: flex;
		align-items: center;
		gap: 9px;
		height: 42px;
		padding: 0 12px;
		border-bottom: 1px solid var(--line);
		color: var(--muted);
	}

	input {
		flex: 1;
		min-width: 0;
		border: 0;
		background: transparent;
		font-size: 14px;
		color: var(--ink);
		cursor: text;
	}

	input::placeholder {
		color: var(--faint);
	}

	input:focus-visible {
		outline: none;
	}

	.list {
		flex: 1;
		min-height: 0;
		padding: 4px 6px 6px;
	}

	.group {
		padding: 8px 8px 2px;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 9px;
		height: 28px;
		padding: 0 8px;
		border-radius: 3px;
		color: var(--muted);
	}

	.row[aria-selected='true'] {
		background: var(--accent-soft);
		color: var(--ink);
	}

	.title {
		flex: none;
		max-width: 62%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--ink);
	}

	.title b {
		font-weight: 650;
		color: var(--accent);
	}

	.detail {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 11.5px;
		color: var(--muted);
	}

	.row kbd {
		margin-left: auto;
	}

	.none {
		padding: 18px 8px;
		color: var(--muted);
	}

	.foot {
		flex: none;
		display: flex;
		align-items: center;
		gap: 14px;
		height: 28px;
		padding: 0 12px;
		border-top: 1px solid var(--line);
		background: var(--well);
		font-size: 11px;
		color: var(--muted);
	}

	.foot kbd {
		margin-right: 3px;
	}

	.count {
		margin-left: auto;
	}
</style>
