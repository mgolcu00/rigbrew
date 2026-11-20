<script lang="ts">
	import { tick } from 'svelte';
	import { app } from '#lib/state.svelte.ts';
	import { chat, sizes, videoLengths, videoSizes, type Message, type Mode, type SizeName } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { guide, type ChecklistId } from '#lib/guide.svelte.ts';
	import { copyText, openExternal } from '#lib/api.ts';
	import { looksEnglish, reader } from '#lib/voice.svelte.ts';
	import { dictation } from '#lib/dictate.svelte.ts';
	import { ago, budgetOf, count, duration, elapsed, familyOf, installedVariant, minute, seconds, size as bytes, speedOf, tierOf, variantName } from '#lib/format.ts';
	import { renderMarkdown } from '#lib/markdown.ts';
	import { machine, mod, nameOf, plainDoes, roleOf, speedAnywhere, speedHere, speedWord, versionLabel } from '#lib/words.ts';
	import type { Package } from '#lib/types.ts';
	import Checklist from '#lib/components/Checklist.svelte';
	import ErrorLine from '#lib/components/ErrorLine.svelte';
	import Hint from '#lib/components/Hint.svelte';
	import Icon, { type IconName } from '#lib/components/Icon.svelte';
	import ImageRow from '#lib/components/ImageRow.svelte';
	import VideoRow from '#lib/components/VideoRow.svelte';
	import InstallControl from '#lib/components/InstallControl.svelte';

	// The chat is a transcript: a left-aligned column of turns, each under one small line that
	// says who spoke (and, in Pro, how: model, build, speed). Pictures are one-line rows. The
	// composer is docked at the bottom, right above the bay. A wide window gets a second pane
	// with the saved chats and, from Standard on, this chat's figures and today's totals.
	//
	// The empty chat depends on the level: Guided asks "What would you like to do?" with four
	// starting points and the checklist; Standard and Pro list what is installed, what can be
	// added and three prompts that run on a click.

	let draft = $state('');
	let input = $state<HTMLTextAreaElement>();
	let scroller = $state<HTMLDivElement>();
	/** True while the view follows the newest message; false once the reader scrolled up. */
	let pinned = $state(true);
	/** A clock for the waiting states. It only ticks while something is being waited for. */
	let now = $state(Date.now());
	let slashAt = $state(0);
	let innerWidth = $state(1200);

	const flags = $derived(ux.flags);
	const models = $derived(app.chatPackages);
	const current = $derived(models.find((m) => m.id === chat.model));
	const imagePkg = $derived(app.imagePackage);
	/** The image package to talk about when none is installed. */
	const imageCatalogue = $derived(app.imageOffer ?? app.packages.find((p) => p.modality === 'image') ?? null);
	const hasImageFamily = $derived(Boolean(imagePkg ?? imageCatalogue));
	const active = $derived(chat.active);
	const messages = $derived(active?.messages ?? []);
	const last = $derived(messages[messages.length - 1]);
	const usable = $derived(models.length > 0 || Boolean(imagePkg));
	/** A saved chat whose messages are still being read: nothing can be sent into it yet. */
	const opening = $derived(Boolean(active && !active.loaded));

	$effect(() => {
		chat.focusRequest;
		input?.focus();
	});

	$effect(() => {
		if (!chat.busy && chat.videosRunning === 0 && dictation.state !== 'recording') return;
		now = Date.now();
		const timer = setInterval(() => (now = Date.now()), 500);
		return () => clearInterval(timer);
	});

	const stick = () => {
		if (pinned && scroller) scroller.scrollTop = scroller.scrollHeight;
	};

	// Follow the answer while it grows, unless the reader scrolled up.
	$effect(() => {
		last?.content;
		last?.state;
		last?.image?.state;
		messages.length;
		tick().then(stick);
	});

	// A chat opened from the list starts at its end.
	$effect(() => {
		chat.activeId;
		pinned = true;
		tick().then(stick);
	});

	function onscroll() {
		if (!scroller) return;
		pinned = scroller.scrollHeight - scroller.scrollTop - scroller.clientHeight < 48;
	}

	function latest() {
		pinned = true;
		scroller?.scrollTo({ top: scroller.scrollHeight, behavior: 'smooth' });
	}

	function resize() {
		if (!input) return;
		input.style.height = 'auto';
		input.style.height = `${Math.min(input.scrollHeight, 200)}px`;
	}

	// --- names, as the level says them ------------------------------------------------------------

	const pkgNamed = (name: string | undefined) => app.packages.find((p) => p.name === name);
	/** "Chat (Steep 4B)" in Guided, "Steep 4B" otherwise, for a chat model's turn. */
	const speaker = (name: string | undefined) => {
		const pkg = pkgNamed(name);
		return pkg ? nameOf(pkg, flags) : (name ?? 'The model');
	};

	// --- slash commands --------------------------------------------------------------------------

	/** Counted by the command's name only. */
	function slashUsed(command: string) {
		guide.count('slash');
		app.track('slash_command', { command });
	}

	interface Command {
		name: string;
		does: string;
		keys?: string;
		run: () => void;
	}

	function setDraft(text: string) {
		draft = text;
		tick().then(() => {
			resize();
			input?.focus();
			input?.setSelectionRange(text.length, text.length);
		});
	}

	const commands = $derived.by((): Command[] => {
		const out: Command[] = [];
		if (hasImageFamily) out.push({ name: '/image', does: imagePkg ? `Describe a picture for ${imagePkg.name}` : 'Describe a picture; Rigbrew offers the install', keys: `${mod}⇧I`, run: () => (chat.setMode('image', 'slash'), setDraft('')) });
		if (models.length > 0) out.push({ name: '/chat', does: `Talk to ${current?.name ?? 'the chat model'}`, keys: `${mod}⇧I`, run: () => (chat.setMode('chat', 'slash'), setDraft('')) });
		if (models.length > 0) out.push({ name: '/model', does: models.length > 1 ? 'Pick the chat model' : `${current?.name ?? ''} is the only chat model installed`, run: () => setDraft('/model ') });
		out.push({ name: '/new', does: 'Start a new chat', keys: `${mod}N`, run: () => (chat.newChat(), setDraft('')) });
		if (chat.engine) out.push({ name: '/unload', does: 'Take the model out of memory', keys: `${mod}⇧U`, run: () => (app.track('unload_clicked', { from: 'slash' }), chat.unload().catch(() => chat.syncEngine()), setDraft('')) });
		out.push({ name: '/models', does: 'Open Models', keys: `${mod}2`, run: () => (setDraft(''), app.go('models')) });
		return out;
	});

	/** What the inline menu offers for the draft as typed, or nothing. */
	const menu = $derived.by((): Command[] => {
		if (!draft.startsWith('/') || draft.includes('\n')) return [];
		const pick = draft.match(/^\/model\s+(.*)$/i);
		if (pick) {
			const word = pick[1].trim().toLowerCase();
			return models
				.filter((m) => m.name.toLowerCase().includes(word) || m.id.includes(word))
				.map((m) => {
					const build = installedVariant(m);
					return {
						name: m.name,
						does: `${build ? `${versionLabel(m, build.id, flags) || bytes(build.accel_mem_mb)} · ${bytes(build.accel_mem_mb)}` : ''}${m.id === chat.model ? ' · in use' : ''}`,
						run: () => {
							chat.model = m.id;
							chat.setMode('chat', 'slash');
							setDraft('');
						}
					};
				});
		}
		if (/\s/.test(draft)) return [];
		const typed = draft.toLowerCase();
		return commands.filter((c) => c.name.startsWith(typed));
	});

	$effect(() => {
		menu.length;
		slashAt = 0;
	});

	// "/image " (or "/picture ") and "/chat " at the start of a message switch the mode and are not sent.
	function oninput() {
		const command = draft.match(/^\/(image|picture|chat)\s/i);
		if (command) {
			const mode: Mode = command[1].toLowerCase() === 'chat' ? 'chat' : 'image';
			if (mode === 'chat' ? models.length > 0 : hasImageFamily) {
				draft = draft.slice(command[0].length);
				chat.setMode(mode, 'slash');
				slashUsed(`/${command[1].toLowerCase()}`);
			}
		}
		resize();
	}

	async function submit() {
		const text = draft;
		if (!text.trim() || chat.busy || opening) return;
		draft = '';
		pinned = true;
		await tick();
		resize();
		chat.send(text);
	}

	function onkeydown(event: KeyboardEvent) {
		if (menu.length > 0) {
			if (event.key === 'ArrowDown' || event.key === 'ArrowUp') {
				event.preventDefault();
				slashAt = (slashAt + (event.key === 'ArrowDown' ? 1 : -1) + menu.length) % menu.length;
				return;
			}
			if ((event.key === 'Enter' && !event.isComposing) || event.key === 'Tab') {
				event.preventDefault();
				const command = menu[slashAt];
				if (command) {
					slashUsed(command.name.startsWith('/') ? command.name : '/model');
					command.run();
				}
				return;
			}
			if (event.key === 'Escape') {
				event.preventDefault();
				setDraft('');
				return;
			}
		}
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing) {
			event.preventDefault();
			submit();
		} else if (event.key === 'Escape' && chat.busy && !app.palette && !app.sheet) {
			chat.stop();
		}
	}

	// Links open in the system browser, never inside the app window; a code block's Copy copies it.
	async function onanswerclick(event: MouseEvent) {
		const target = event.target as HTMLElement;
		const button = target.closest<HTMLElement>('[data-copy]');
		if (button) {
			await copyText(button.closest('.code')?.querySelector('pre')?.textContent ?? '');
			button.textContent = 'Copied';
			setTimeout(() => (button.textContent = 'Copy'), 1400);
			return;
		}
		const link = target.closest('a');
		if (!link) return;
		event.preventDefault();
		openExternal(link.href);
	}

	async function readAloud(message: Message) {
		const voice = app.voicePackage;
		if (!voice) return;
		await reader.toggle(message.id, voice.id, message.content);
		if (reader.error) app.say(`Could not read the answer aloud. ${reader.error}`);
	}

	/** First press opens the microphone, the second writes down what was said into the box. */
	async function dictate() {
		if (dictation.state === 'recording') {
			const text = await dictation.stop();
			if (text) setDraft(draft.trim() ? `${draft.trimEnd()} ${text}` : text);
			if (dictation.error) app.say(`Dictation did not work. ${dictation.error}`);
			return;
		}
		await dictation.start();
		if (dictation.error) app.say(dictation.error);
	}

	let copiedId = $state(0);
	async function copyAnswer(message: Message) {
		await copyText(message.content);
		copiedId = message.id;
		setTimeout(() => (copiedId = 0), 1400);
	}

	function waitingLabel(message: Message) {
		if (flags.techDetails) {
			const name = message.modelName ?? 'the model';
			if (message.switching) return `Switching to ${name}`;
			return chat.engine ? `${name} is reading` : `Loading ${name}`;
		}
		const name = speaker(message.modelName);
		if (message.switching) return `Moving ${name} into memory`;
		return chat.engine ? `${name} is thinking` : `Loading ${name} into memory`;
	}

	const waitedFor = (message: Message) => (message.startedAt ? (now - message.startedAt) / 1000 : 0);

	// --- the chat's own head: its name (click to rename) and Delete --------------------------------

	let renaming = $state(false);
	let newTitle = $state('');
	let titleField = $state<HTMLInputElement>();

	async function startRename() {
		if (!active) return;
		newTitle = active.title;
		renaming = true;
		await tick();
		titleField?.select();
	}

	function finishRename(keep: boolean) {
		if (keep && active) chat.rename(active.id, newTitle);
		renaming = false;
	}

	// --- the empty transcript ------------------------------------------------------------------------

	const system = $derived(app.system);
	const budget = $derived(system ? budgetOf(system) : null);
	const ready = $derived(app.packages.filter((p) => p.installed.length > 0));
	const absent = $derived(app.packages.filter((p) => p.installed.length === 0));

	/** Guided: four ways to start, each acting at once. */
	interface Start {
		id: string;
		title: string;
		does: string;
		icon: IconName;
		run: () => void;
	}

	function begin(mode: Mode, text: string) {
		chat.newChat();
		chat.setMode(mode, 'click');
		setDraft(text);
	}

	const starts = $derived.by((): Start[] => {
		const out: Start[] = [];
		const talk = current ? nameOf(current, flags) : 'the chat model';
		if (current) out.push({ id: 'ask', title: 'Ask anything', does: `Questions, ideas, advice. ${talk} answers.`, icon: 'chat', run: () => begin('chat', '') });
		if (imagePkg) {
			out.push({ id: 'picture', title: 'Make a picture', does: `Describe it and ${nameOf(imagePkg, flags)} draws it.`, icon: 'image', run: () => begin('image', '') });
		} else if (imageCatalogue && imageCatalogue.recommendation.verdict === 'run') {
			const rec = imageCatalogue.recommendation;
			out.push({ id: 'picture', title: 'Make a picture', does: `Needs ${nameOf(imageCatalogue, flags)}, a ${bytes(rec.disk_mb)} download. Install it in Models.`, icon: 'image', run: () => app.showModel(imageCatalogue.id) });
		}
		if (app.videoPackage) out.push({ id: 'clip', title: 'Make a short video', does: `Describe what moves. ${nameOf(app.videoPackage, flags)} makes a clip in a few minutes.`, icon: 'video', run: () => begin('video', '') });
		if (current) {
			out.push({ id: 'write', title: 'Help me write…', does: 'An email, a message, a short text. Say who it is for.', icon: 'compose', run: () => begin('chat', 'Help me write ') });
			out.push({ id: 'explain', title: 'Explain something simply', does: 'Any topic, in plain words, as short as you like.', icon: 'bolt', run: () => begin('chat', 'Explain simply: ') });
		}
		return out;
	});

	/** The checklist's steps, done on a click. */
	function step(id: ChecklistId) {
		if (id === 'ask') begin('chat', '');
		else if (id === 'picture') {
			if (imagePkg) begin('image', '');
			else if (imageCatalogue) app.showModel(imageCatalogue.id);
		} else if (id === 'models') app.go('models');
		else if (id === 'profile') app.go('profile');
		else if (id === 'search') app.palette = true;
	}

	/** The chat model to offer when nothing is installed yet. */
	const firstChat = $derived(app.packages.find((p) => p.modality === 'chat' && p.recommendation.verdict === 'run') ?? null);

	interface Example {
		text: string;
		mode: Mode;
		route: string;
	}

	// Three things that run as they are: a click sends them.
	const examples = $derived.by((): Example[] => {
		const out: Example[] = [];
		if (current) out.push({ text: 'Explain what quantizing a model to 5 bits gives up, in five lines', mode: 'chat', route: current.name });
		if (imagePkg) out.push({ text: 'A workbench by a window, an open aluminium frame with one part seated in it, morning light', mode: 'image', route: imagePkg.name });
		if (current && imagePkg) out.push({ text: 'Draw a small robot repairing a radio, then describe it in one sentence', mode: 'chat', route: `${current.name} → ${imagePkg.name}` });
		if (current && out.length < 3) out.push({ text: 'Write a shell one-liner that lists the ten largest files under the current folder', mode: 'chat', route: current.name });
		if (current && out.length < 3) out.push({ text: 'Give me three names for a small weather app and say why', mode: 'chat', route: current.name });
		return out.slice(0, 3);
	});

	function runExample(example: Example) {
		if (chat.busy) return;
		chat.newChat();
		chat.setMode(example.mode, 'click');
		pinned = true;
		chat.send(example.text);
	}

	const headline = $derived.by(() => {
		const running = ready.filter((p) => familyOf(p).runs);
		if (running.length === 0) return 'Nothing is installed yet.';
		// Pro names the models; Standard says what is ready to do.
		const names = flags.techDetails ? running.map((p) => p.name) : [...new Set(running.map((p) => roleOf(p).toLowerCase()))];
		const list = names.length === 1 ? names[0] : `${names.slice(0, -1).join(', ')} and ${names[names.length - 1]}`;
		if (flags.techDetails) return `${list} ${names.length === 1 ? 'is' : 'are'} ready.`;
		return `Ready for ${list}.`;
	});

	/** Speed for the roster: a word with the number on hover (Standard), the number (Pro). */
	function pace(pkg: Package) {
		const build = installedVariant(pkg);
		if (!build) return { text: '', title: '' };
		if (flags.speedNumbers) return { text: speedOf(build.metrics), title: 'Measured tokens per second' };
		const here = speedHere(build, system);
		const any = here ?? speedAnywhere(build);
		if (!flags.speedWords || any === null) return { text: '', title: '' };
		return { text: speedWord(any), title: `${any.toFixed(1)} tokens per second${here === null ? ', measured on another machine' : ''}. A token is a short word or part of one.` };
	}

	// --- the side pane -----------------------------------------------------------------------------

	const stats = $derived.by(() => {
		const c = chat.active;
		if (!c || !c.loaded) return null;
		const answers = c.messages.filter((m) => m.role === 'assistant' && m.content);
		const rated = answers.filter((m) => m.tokensPerSecond);
		return {
			sent: c.messages.filter((m) => m.role === 'user').length,
			answers: answers.length,
			tokens: answers.reduce((n, m) => n + (m.tokens ?? 0), 0),
			images: c.messages.filter((m) => m.image?.state === 'done').length,
			speed: rated.length ? rated.reduce((n, m) => n + m.tokensPerSecond!, 0) / rated.length : 0,
			startedAt: c.startedAt
		};
	});

	const today = $derived(app.usage?.today ?? null);
	const showPane = $derived(app.pane && innerWidth >= 1080);

	const keys: [string, string][] = [
		[`${mod}K`, 'Search and commands'],
		[`${mod}N`, 'New chat'],
		[`${mod}⇧I`, 'Chat or Picture'],
		[`${mod}⇧U`, 'Unload the model'],
		[`${mod}\\`, 'This pane'],
		[`${mod}/`, 'Help'],
		['/', 'Commands in the composer'],
		['Esc', 'Stop the answer']
	];

	const placeholder = $derived.by(() => {
		if (!usable) return 'Install a chat model to start';
		if (opening) return 'Opening the chat…';
		if (chat.mode === 'image') {
			const name = (imagePkg ?? imageCatalogue)?.name ?? 'the picture model';
			return flags.plainNames ? 'Describe the picture you want' : `Describe a picture for ${name}`;
		}
		if (chat.mode === 'video') {
			return flags.plainNames ? 'Describe the clip you want: what moves, and how' : `Describe a clip for ${app.videoPackage?.name ?? 'the video model'}`;
		}
		if (flags.slashHints) return `Message ${current?.name ?? ''}, or / for commands`;
		if (flags.plainNames) return 'Ask anything';
		return `Message ${current?.name ?? ''}`;
	});
</script>

<svelte:window bind:innerWidth />

<section class="chat" class:paned={showPane} class:slim={!flags.paneFigures} aria-label="Chat">
	<div class="main">
		<p class="sr" aria-live="polite">{chat.announce}</p>
		<div class="thread scroll" bind:this={scroller} {onscroll}>
			<div class="column" class:home={!active} class:plain={!active && flags.plainStart}>
				{#if !active && flags.plainStart}
					<!-- Guided: one question, four ways to start, the checklist, what is installed. -->
					<div class="first">
						{#if usable}
							<h1>What would you like to do?</h1>
							<ul class="starts">
								{#each starts as s (s.id)}
									<li>
										<button onclick={() => (app.track('starter_clicked', { starter: s.id }), s.run())} disabled={chat.busy}>
											<span class="ico"><Icon name={s.icon} size={16} /></span>
											<span class="st">{s.title}</span>
											<span class="sd">{s.does}</span>
											<Icon name="chevron" size={13} />
										</button>
									</li>
								{/each}
							</ul>
						{:else}
							<h1>Install a chat model to start</h1>
							<p class="lede">
								A model is what answers you. {#if firstChat}{nameOf(firstChat, flags)} suits this {machine}{firstChat.recommendation.verdict === 'run' ? `: a ${bytes(firstChat.recommendation.disk_mb)} download` : ''}.{/if}
								<Hint topic="models" />
							</p>
							{#if firstChat}<p class="row"><InstallControl pkg={firstChat} primary from="chat" /> <button class="btn quiet" onclick={() => app.go('models')}>See all models</button></p>{/if}
						{/if}

						{#if flags.checklist}<div class="check"><Checklist act={step} /></div>{/if}

						{#if ready.length > 0}
							<h2 class="rule label">On this {machine}</h2>
							<ul class="owned">
								{#each ready as pkg (pkg.id)}
									{@const family = familyOf(pkg)}
									<li>
										<span class="ico"><Icon name={family.icon} size={15} /></span>
										<button class="oname" onclick={() => app.showModel(pkg.id)} title="Show in Models">{nameOf(pkg, flags)}</button>
										<span class="odoes">{family.runs ? plainDoes(pkg) : `${plainDoes(pkg)} Download only for now.`}</span>
										{#if chat.engine?.package === pkg.id}<span class="tag accent" title="In memory and ready to answer at once">ready</span>{/if}
									</li>
								{/each}
							</ul>
						{/if}
						{#if absent.length > 0}
							<p class="addable">
								Can be added: {#each absent as pkg, i (pkg.id)}{i > 0 ? ', ' : ''}<button class="link" onclick={() => app.showModel(pkg.id)}>{nameOf(pkg, flags)}</button>{/each}.
								<button class="link" onclick={() => app.go('models')}>See all models</button>
							</p>
						{/if}

						{#if chat.conversations.length > 0 && !showPane}
							<h2 class="rule label">Recent chats</h2>
							<ul class="recent">
								{#each chat.conversations.slice(0, 5) as c (c.id)}
									<li><button onclick={() => chat.open(c.id, 'recent')}><span class="say">{c.title}</span><span class="num">{ago(c.updatedAt, now)}</span></button></li>
								{/each}
							</ul>
						{/if}

						<p class="fine">
							Everything runs on this {machine}. The first answer takes a few seconds longer while the model loads.
							What you type is collected as usage data, with your consent; <button class="link" onclick={() => app.openPrivacy()}>see what is sent</button>.
						</p>
					</div>
				{:else if !active}
					<div class="start">
						<div class="lead">
							{#if system && budget}
								<p class="machine num selectable">
									{#if flags.techDetails}{system.cpu.brand} · {tierOf(system).label} · {bytes(budget.mb)} for models on the {budget.where}{:else}{system.cpu.brand} · {bytes(budget.mb)} of memory for models{/if}
								</p>
							{/if}
							<h1>{headline}</h1>
						</div>

						<div class="have">
							{#if ready.length > 0}
								<h2 class="rule label">On this {flags.techDetails ? 'machine' : machine} now</h2>
								<table class="roster" class:named={flags.versionNames}>
									<tbody>
										{#each ready as pkg (pkg.id)}
											{@const family = familyOf(pkg)}
											{@const build = installedVariant(pkg)}
											{@const speed = pace(pkg)}
											<tr>
												<th scope="row"><button class="name" onclick={() => app.showModel(pkg.id)} title="Show {pkg.name} in Models">{pkg.name}</button></th>
												<td class="num build">{build ? versionLabel(pkg, build.id, flags) : ''}</td>
												<td><span class="tag">{flags.techDetails ? family.does : roleOf(pkg)}</span></td>
												<td class="num" title="Memory it takes while loaded">{build ? bytes(build.accel_mem_mb) : ''}</td>
												<td class="num dim" title={speed.title}>{speed.text}</td>
												<td class="can">{family.runs ? (flags.techDetails ? family.can : plainDoes(pkg)) : 'Downloaded; running it in Rigbrew comes later'}</td>
												<td class="state">
													{#if chat.engine?.package === pkg.id}<span class="tag accent">{flags.techDetails ? 'in memory' : 'loaded'}</span>{:else if pkg.id === chat.model && family.runs}<span class="tag">chat model</span>{/if}
												</td>
											</tr>
										{/each}
									</tbody>
								</table>
							{/if}

							{#if absent.length > 0}
								<h2 class="rule label">{ready.length > 0 ? 'Can be added' : 'Install a model to start'}</h2>
								<table class="roster" class:named={flags.versionNames}>
									<tbody>
										{#each absent as pkg (pkg.id)}
											{@const family = familyOf(pkg)}
											{@const rec = pkg.recommendation}
											<tr>
												<th scope="row"><button class="name" onclick={() => app.showModel(pkg.id)}>{pkg.name}</button></th>
												<td class="num build">{'variant' in rec ? versionLabel(pkg, rec.variant, flags) : ''}</td>
												<td><span class="tag">{flags.techDetails ? family.does : roleOf(pkg)}</span></td>
												<td class="num">{rec.verdict === 'run' ? bytes(rec.memory_mb) : ''}</td>
												<td class="can" colspan="2">{flags.techDetails ? family.can : plainDoes(pkg).replace(/\.$/, '')}{family.runs ? '' : ' (download only for now)'}</td>
												<td class="state">
													{#if rec.verdict === 'run'}<InstallControl {pkg} primary={!usable && pkg.modality === 'chat'} from="chat" />{:else}<span class="muted">does not fit</span>{/if}
												</td>
											</tr>
										{/each}
									</tbody>
								</table>
							{/if}
						</div>

						<div class="do">
							{#if flags.checklist}<div class="check"><Checklist act={step} /></div>{/if}
							{#if examples.length > 0}
								<h2 class="rule label">{flags.techDetails ? 'Run one' : 'Try one'}</h2>
								<ol class="examples">
									{#each examples as example, i (example.text)}
										<li>
											<button onclick={() => runExample(example)} disabled={chat.busy}>
												<span class="n num">{i + 1}</span>
												<span class="say">{example.text}</span>
												{#if flags.slashHints}<span class="route num">{example.mode === 'image' ? '/image' : 'chat'} → {example.route}</span>{:else if example.mode === 'image'}<span class="route">picture</span>{/if}
												<Icon name="return" size={14} />
											</button>
										</li>
									{/each}
								</ol>
							{/if}

							{#if chat.conversations.length > 0 && !showPane}
								<h2 class="rule label">Recent chats</h2>
								<ul class="recent">
									{#each chat.conversations.slice(0, 5) as c (c.id)}
										<li><button onclick={() => chat.open(c.id, 'recent')}><span class="say">{c.title}</span><span class="num">{ago(c.updatedAt, now)}</span></button></li>
									{/each}
								</ul>
							{/if}

							<p class="fine">
								Everything runs on this {flags.techDetails ? 'machine' : machine}. Prompts are logged as usage data;
								<button class="link" onclick={() => app.openPrivacy()}>see what is queued</button>.
							</p>
						</div>
					</div>
				{:else}
					<header class="head">
						{#if renaming}
							<input
								class="rename"
								bind:this={titleField}
								bind:value={newTitle}
								aria-label="Chat name"
								maxlength="64"
								onkeydown={(e) => {
									if (e.key === 'Enter') finishRename(true);
									else if (e.key === 'Escape') {
										e.stopPropagation();
										finishRename(false);
									}
								}}
								onblur={() => finishRename(true)}
							/>
						{:else}
							<button class="title" onclick={startRename} title="Rename this chat" aria-label="Chat name: {active.title}. Rename">
								<span class="tname">{active.title}</span>
								<Icon name="compose" size={13} />
							</button>
						{/if}
						<span class="grow"></span>
						{#if active.loaded}<span class="num when" title={new Date(active.startedAt).toLocaleString()}>{ago(active.startedAt, now)}</span>{/if}
						<button class="tool" onclick={() => chat.remove(active.id)} disabled={chat.busy} title="Delete this chat" aria-label="Delete this chat"><Icon name="trash" size={15} /></button>
					</header>

					{#if opening}
						{#if active.problem}
							<ErrorLine error={active.problem} lead="This chat could not be opened." onretry={() => chat.retryOpen(active.id)} where="open_chat" />
						{:else}
							<p class="waiting num opening" aria-live="polite"><span class="dot"></span>Opening the chat</p>
						{/if}
					{/if}

					<ol class="turns">
						{#each messages as message (message.id)}
							{#if message.role === 'user'}
								<li class="turn you">
									<p class="meta">
										<span class="label ink">You</span>
										{#if message.mode === 'image'}<span class="tag">picture</span>{/if}
										{#if message.at}<time class="num">{minute(message.at)}</time>{/if}
									</p>
									<div class="said selectable">{message.content}</div>
								</li>
							{:else if message.role === 'image' && message.image}
								<li class="turn"><ImageRow job={message.image} {now} onload={stick} /></li>
							{:else if message.role === 'video' && message.video}
								<li class="turn"><VideoRow job={message.video} {now} /></li>
							{:else if message.role === 'offer' && message.offer}
								<li class="turn offer">
									{#if imagePkg}
										<p class="meta"><span class="label ink">{nameOf(imagePkg, flags)} is installed</span></p>
										<p>It can make the picture you asked for now.</p>
										<p class="row"><button class="btn primary" onclick={() => chat.fulfil(message)} disabled={chat.busy}><Icon name="image" size={14} /> Make the picture</button></p>
									{:else if imageCatalogue}
										{@const rec = imageCatalogue.recommendation}
										<p class="meta"><span class="label ink">No picture model installed</span>{#if rec.verdict === 'run' && !app.installs[imageCatalogue.id]}<span class="tag fit">fits</span>{/if}</p>
										<p>
											Pictures are made by {nameOf(imageCatalogue, flags)}, which installs separately.
											{#if rec.verdict === 'run'}It is a <span class="num">{bytes(rec.disk_mb)}</span> download and uses <span class="num">{bytes(rec.memory_mb)}</span> of memory while it works.{/if}
											<Hint topic="pictures" />
										</p>
										<p class="row">
											<InstallControl pkg={imageCatalogue} primary from="chat" />
											{#if models.length > 0 && !message.offer.callId && message === last}
												<button class="btn quiet" onclick={() => chat.answerInText()} disabled={chat.busy}>Answer in words instead</button>
											{/if}
										</p>
									{:else}
										<p class="meta"><span class="label ink">No picture model available</span></p>
										<p>The catalogue has no picture model yet.</p>
									{/if}
								</li>
							{:else if message.role === 'assistant' && (message.content || message.state !== 'done')}
								<li class="turn model">
									<p class="meta">
										<span class="label ink">{flags.techDetails ? message.modelName : speaker(message.modelName)}</span>
										<span class="num">
											{#if flags.techDetails}
												{#if message.variant}{variantName(message.variant)}{/if}
												{#if message.state === 'streaming' && chat.rate > 0}<span class="sep">{chat.rate.toFixed(1)} tok/s</span>{/if}
												{#if message.state === 'done' || message.state === 'stopped'}
													{#if message.tokensPerSecond}<span class="sep">{message.tokensPerSecond.toFixed(1)} tok/s</span>{/if}
													{#if message.tokens}<span class="sep">{message.tokens} tok</span>{/if}
													{#if message.took}<span class="sep">{seconds(message.took)}</span>{/if}
												{/if}
											{:else if flags.speedWords && message.state === 'done' && message.tokensPerSecond}
												<span title="{message.tokensPerSecond.toFixed(1)} tokens per second{message.tokens ? `, ${message.tokens} tokens` : ''}">{speedWord(message.tokensPerSecond)}</span>{#if message.took}<span class="sep">{seconds(message.took)}</span>{/if}
											{/if}
											{#if message.state === 'stopped'}<span class="sep">stopped</span>{/if}
										</span>
										{#if message.at && !flags.techDetails}<time class="num">{minute(message.at)}</time>{/if}
										{#if message.state === 'done' && message.content}
											{#if app.voicePackage}
												<button
													class="mini"
													class:on={reader.id === message.id}
													onclick={() => readAloud(message)}
													title={reader.id === message.id
														? 'Stop reading'
														: looksEnglish(message.content)
															? `Read this answer aloud with ${app.voicePackage.name}`
															: `${app.voicePackage.name} reads English; this answer is in another language and will sound wrong`}
													>{reader.id === message.id ? (reader.state === 'preparing' ? 'Preparing…' : 'Stop') : looksEnglish(message.content) ? 'Read aloud' : 'Read aloud (English)'}</button
												>
											{/if}
											<button class="mini" onclick={() => copyAnswer(message)} title="Copy this answer">{copiedId === message.id ? 'Copied' : 'Copy'}</button>
										{/if}
									</p>
									{#if message.state === 'waiting'}
										<p class="waiting num" aria-live="polite">
											<span class="dot"></span>
											{waitingLabel(message)}
											<span class="clock">{elapsed(waitedFor(message))}</span>
											{#if !flags.techDetails && !chat.engine}<Hint topic="first-answer" />{/if}
										</p>
									{:else if message.content}
										<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
										<div class="prose selectable" class:writing={message.state === 'streaming'} aria-busy={message.state === 'streaming'} onclick={onanswerclick}>{@html renderMarkdown(message.content)}</div>
									{/if}
									{#if message.state === 'failed'}
										<ErrorLine error={message.error} lead="No answer." onretry={message === last && !chat.busy ? () => chat.retry() : undefined} />
									{/if}
									{#if message.toolCalls?.length && message.state === 'done'}
										{#each message.toolCalls as call (call.id)}
											{#if flags.techDetails}
												<p class="called num">called <span class="fn">{call.name}</span></p>
											{:else if call.name === 'generate_image'}
												<p class="called">Asked {imagePkg ? nameOf(imagePkg, flags) : 'the picture model'} to make a picture</p>
											{/if}
										{/each}
									{/if}
								</li>
							{/if}
						{/each}
					</ol>
				{/if}
			</div>
		</div>

		<form
			class="dock"
			onsubmit={(e) => {
				e.preventDefault();
				submit();
			}}
		>
			<div class="column" class:home={!active} class:plain={!active && flags.plainStart}>
				{#if !pinned && messages.length > 0}
					<button type="button" class="latest" onclick={latest}><Icon name="down" size={13} /> Latest</button>
				{/if}

				{#if menu.length > 0}
					<ul class="slash" role="listbox" aria-label="Commands">
						{#each menu as command, i (command.name)}
							<!-- svelte-ignore a11y_click_events_have_key_events -->
							<li role="option" aria-selected={i === slashAt} onclick={() => (slashUsed(command.name.startsWith('/') ? command.name : '/model'), command.run())} onmousemove={() => (slashAt = i)}>
								<span class="cmd num">{command.name}</span>
								<span class="does">{command.does}</span>
								{#if command.keys && flags.keyHints}<kbd>{command.keys}</kbd>{/if}
							</li>
						{/each}
					</ul>
				{/if}

				<div class="composer" class:off={!usable} data-tour="composer">
					<textarea
						bind:this={input}
						bind:value={draft}
						{oninput}
						{onkeydown}
						rows="1"
						disabled={!usable || opening}
						spellcheck="false"
						{placeholder}
						aria-label={chat.mode === 'image' ? 'Picture description' : chat.mode === 'video' ? 'Clip description' : 'Message'}
					></textarea>
					<div class="tools">
						{#if hasImageFamily && models.length > 0}
							<div class="chips" role="radiogroup" aria-label="What to make" data-tour="mode">
								<button type="button" role="radio" aria-checked={chat.mode === 'chat'} onclick={() => chat.setMode('chat', 'click')} disabled={chat.busy} title="Chat: answers in words">Chat</button>
								<button type="button" role="radio" aria-checked={chat.mode === 'image'} onclick={() => chat.setMode('image', 'click')} disabled={chat.busy} title="Picture: makes an image from your description{flags.keyHints ? ` (${mod}⇧I, or type /image)` : ''}">Picture</button>
								{#if app.videoPackage}
									<button type="button" role="radio" aria-checked={chat.mode === 'video'} onclick={() => chat.setMode('video', 'click')} disabled={chat.busy} title="Video: makes a short clip from your description, in a few minutes">Video</button>
								{/if}
							</div>
						{/if}
						{#if chat.mode === 'video'}
							<div class="chips" role="radiogroup" aria-label="Clip shape">
								{#each Object.entries(videoSizes) as [name, shape] (name)}
									<button type="button" role="radio" aria-checked={chat.videoSize === name} onclick={() => (chat.videoSize = name as SizeName)} disabled={chat.busy} title="{shape.label}{flags.techDetails ? `, ${shape.width} × ${shape.height}` : ''}" aria-label={shape.label}>
										<span class="shape" style:aspect-ratio="{shape.width} / {shape.height}"></span>
									</button>
								{/each}
							</div>
							<div class="chips" role="radiogroup" aria-label="Clip length">
								{#each videoLengths as l (l.frames)}
									<button type="button" role="radio" class="num" aria-checked={chat.videoFrames === l.frames} onclick={() => (chat.videoFrames = l.frames)} disabled={chat.busy} title="{l.label}{flags.techDetails ? `, ${l.frames} frames` : ''}">{l.label}</button>
								{/each}
							</div>
							<span class="to">{#if flags.techDetails}<span class="num">→ {app.videoPackage?.name} · {videoSizes[chat.videoSize].width}×{videoSizes[chat.videoSize].height} · {chat.videoFrames} frames</span>{:else}A few minutes{/if}</span>
						{:else if chat.mode === 'image'}
							<div class="chips" role="radiogroup" aria-label="Picture shape">
								{#each Object.entries(sizes) as [name, shape] (name)}
									<button type="button" role="radio" aria-checked={chat.size === name} onclick={() => (chat.size = name as SizeName)} disabled={chat.busy} title="{shape.label}{flags.techDetails ? `, ${shape.width} × ${shape.height}` : ''}" aria-label={shape.label}>
										<span class="shape" style:aspect-ratio="{shape.width} / {shape.height}"></span>
									</button>
								{/each}
							</div>
							{#if flags.techDetails}
								<span class="num to">→ {(imagePkg ?? imageCatalogue)?.name} · {sizes[chat.size].width}×{sizes[chat.size].height} · 8 steps</span>
							{:else}
								<span class="to">{sizes[chat.size].label}{#if imagePkg} · {nameOf(imagePkg, flags)}{/if}</span>
							{/if}
						{:else if current}
							{@const build = installedVariant(current)}
							{@const version = build ? versionLabel(current, build.id, flags) : ''}
							<button type="button" class="to pick" class:num={flags.techDetails} onclick={() => setDraft('/model ')} disabled={models.length < 2} title={models.length > 1 ? 'Pick the chat model' : 'The chat model that answers'}>
								{#if flags.techDetails}→ {current.name}{build ? ` · ${variantName(build.id)}` : ''}{:else}{nameOf(current, flags)}{version ? ` · ${version}` : ''}{/if}
							</button>
						{/if}
						<span class="grow"></span>
						{#if app.listenPackage}
							<button
								type="button"
								class="mic"
								class:live={dictation.state === 'recording'}
								onclick={dictate}
								disabled={dictation.state === 'working' || !usable}
								title={dictation.state === 'recording' ? 'Stop and write it down' : dictation.state === 'working' ? `${app.listenPackage.name} is writing it down` : `Talk instead of typing: ${app.listenPackage.name} writes it down on this ${machine}`}
								aria-label={dictation.state === 'recording' ? 'Stop dictating' : 'Dictate'}
							>
								<Icon name="listening" size={14} />
								{#if dictation.state === 'recording'}<span class="num">{elapsed((now - dictation.startedAt) / 1000)}</span>{:else if dictation.state === 'working'}<span>Writing…</span>{/if}
							</button>
						{/if}
						{#if chat.busy}
							<button type="button" class="go stop" onclick={() => chat.stop()} title="Stop{flags.keyHints ? ' (Esc)' : ''}">Stop{#if flags.keyHints}&nbsp;<kbd>esc</kbd>{/if}</button>
						{:else}
							<button type="submit" class="go" disabled={!draft.trim() || menu.length > 0 || opening} title="Send (Enter). Shift+Enter starts a new line.">{chat.mode === 'chat' ? 'Send' : 'Make'}{#if flags.keyHints}&nbsp;<kbd>↵</kbd>{/if}</button>
						{/if}
					</div>
				</div>
			</div>
		</form>
	</div>

	{#if showPane}
		<aside class="pane scroll" aria-label="Chats{flags.paneFigures ? ', this chat and today' : ''}">
			{#if stats && flags.paneFigures}
				<section>
					<h2 class="rule label">This chat</h2>
					<p class="ptitle selectable">{chat.active?.title}</p>
					<dl class="figures">
						<div><dt>Sent</dt><dd class="num">{stats.sent}</dd></div>
						<div><dt>Answers</dt><dd class="num">{stats.answers}</dd></div>
						{#if flags.techDetails}<div><dt>Tokens out</dt><dd class="num">{count(stats.tokens)}</dd></div>{/if}
						<div><dt>Pictures</dt><dd class="num">{stats.images}</dd></div>
						{#if flags.techDetails}
							<div><dt>Mean speed</dt><dd class="num">{stats.speed ? `${stats.speed.toFixed(1)} tok/s` : 'none yet'}</dd></div>
						{:else if stats.speed}
							<div><dt>Speed</dt><dd class="num" title="{stats.speed.toFixed(1)} tokens per second">{speedWord(stats.speed)}</dd></div>
						{/if}
						<div><dt>Started</dt><dd class="num">{ago(stats.startedAt, now)}</dd></div>
					</dl>
				</section>
			{/if}

			<section>
				<h2 class="rule label">Chats</h2>
				<ul class="chats">
					<li>
						<button class="open" class:current={chat.activeId === ''} onclick={() => chat.newChat()}><span class="say">New chat</span>{#if flags.keyHints}<kbd>{mod}N</kbd>{/if}</button>
					</li>
					{#each chat.conversations as c (c.id)}
						<li>
							<button class="open" class:current={chat.activeId === c.id} onclick={() => chat.open(c.id, 'pane')} title={c.title}><span class="say">{c.title}</span><span class="num">{ago(c.updatedAt, now).replace(' ago', '')}</span></button>
							<button class="del" onclick={() => chat.remove(c.id)} disabled={chat.busy && chat.activeId === c.id} title="Delete “{c.title}”" aria-label="Delete chat “{c.title}”"><Icon name="trash" size={13} /></button>
						</li>
					{/each}
				</ul>
				{#if chat.persist === 'no'}
					<p class="quiet">Chats are kept until Rigbrew quits: this version of the background service cannot save them yet.</p>
				{:else if chat.saveProblem}
					<p class="quiet problem" title={chat.saveProblem}>The last change to a chat could not be saved. Rigbrew tries again after the next answer.</p>
				{:else if chat.conversations.length === 0}
					<p class="quiet">Chats are saved on this {machine} as you go.</p>
				{/if}
			</section>

			{#if today && flags.paneFigures}
				<section>
					<h2 class="rule label">Today</h2>
					<dl class="figures">
						<div><dt>Messages</dt><dd class="num">{count(today.chat_messages)}</dd></div>
						<div><dt>Pictures</dt><dd class="num">{count(today.images)}</dd></div>
						{#if flags.techDetails}<div><dt>Tokens</dt><dd class="num">{count(today.tokens_in + today.tokens_out)}</dd></div>{/if}
						<div><dt>{flags.techDetails ? 'Generating' : 'Time working'}</dt><dd class="num">{duration(today.seconds_generating)}</dd></div>
					</dl>
					<p class="quiet"><button class="link" onclick={() => app.go('profile')}>All activity in your profile</button></p>
				</section>
			{/if}

			{#if flags.keysPane}
				<section>
					<h2 class="rule label">Keys</h2>
					<dl class="keys">
						{#each keys as [key, what] (key)}
							<div><dt><kbd>{key}</kbd></dt><dd>{what}</dd></div>
						{/each}
					</dl>
				</section>
			{/if}
		</aside>
	{/if}
</section>

<style>
	.chat {
		height: 100%;
		min-height: 0;
		display: grid;
		grid-template-columns: minmax(0, 1fr);
	}

	.main {
		min-width: 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
	}

	/* The last lines fade out above the composer instead of being cut at its edge, and the
	   end of the column clears that fade, so nothing is ever read half behind the composer. */
	.thread {
		flex: 1;
		min-height: 0;
		-webkit-mask-image: linear-gradient(to bottom, #000 calc(100% - 28px), transparent);
		mask-image: linear-gradient(to bottom, #000 calc(100% - 28px), transparent);
	}

	.thread > .column {
		padding-bottom: 32px;
	}

	.column {
		position: relative;
		width: 100%;
		max-width: 860px;
		margin: 0 auto;
		padding: 0 var(--gutter);
	}

	/* --- the empty transcript --- */
	.start {
		padding: 28px 0 24px;
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		column-gap: 44px;
	}

	.do .rule:first-child {
		margin-top: 22px;
	}

	/* A wide window puts what can be run next to what is installed. */
	@media (min-width: 1500px) {
		.column.home {
			max-width: 1320px;
		}

		.column.home .composer,
		.column.home .slash {
			max-width: 820px;
		}

		.start {
			grid-template-columns: minmax(0, 13fr) minmax(0, 10fr);
		}

		.lead {
			grid-column: 1 / -1;
		}
	}

	.machine {
		font-size: 12px;
		color: var(--muted);
	}

	.start h1 {
		margin-top: 6px;
		font-size: 19px;
		text-wrap: balance;
	}

	.start .rule {
		margin-top: 22px;
	}

	.roster {
		width: 100%;
		border-collapse: collapse;
		table-layout: fixed;
	}

	/* Both lists share one set of columns, so the figures line up down the page. */
	.roster th {
		width: 112px;
	}

	.roster td:nth-child(2) {
		width: 46px;
	}

	/* Friendly version names ("Best quality") are longer than build ids. */
	.roster.named td:nth-child(2) {
		width: 104px;
	}

	.roster td:nth-child(3) {
		width: 78px;
	}

	.roster td:nth-child(4) {
		width: 68px;
	}

	.roster td.dim {
		width: 88px;
	}

	.roster td.state {
		width: 104px;
	}

	.roster td.state:has(:global(.btn)) {
		width: 136px;
	}

	.roster tr {
		border-bottom: 1px solid var(--line-soft);
	}

	.roster th,
	.roster td {
		height: 30px;
		padding: 0 14px 0 0;
		text-align: left;
		font-weight: 400;
		white-space: nowrap;
	}

	.roster .name {
		font: 600 12.5px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.roster .name:hover {
		color: var(--accent);
	}

	.roster .num {
		font-size: 12px;
	}

	.roster .build,
	.roster .dim {
		color: var(--muted);
	}

	.roster .can {
		overflow: hidden;
		text-overflow: ellipsis;
		color: var(--muted);
	}

	.roster .state {
		padding-right: 0;
		text-align: right;
	}

	.examples li,
	.recent li {
		border-bottom: 1px solid var(--line-soft);
	}

	.examples button,
	.recent button {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		min-height: 30px;
		padding: 5px 6px 5px 0;
		transition: background-color var(--quick);
	}

	.examples button:hover,
	.recent button:hover {
		background: var(--hover);
	}

	.examples button:hover :global(svg) {
		color: var(--accent);
	}

	.examples :global(svg) {
		color: var(--faint);
	}

	.n {
		flex: none;
		width: 16px;
		font-size: 11.5px;
		color: var(--faint);
		text-align: right;
	}

	.say {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.route,
	.recent .num {
		flex: none;
		font-size: 11.5px;
		color: var(--muted);
	}

	.fine {
		margin-top: 18px;
		font-size: 12px;
		color: var(--muted);
	}

	/* --- the transcript --- */
	.turns {
		padding: 14px 0 20px;
		display: flex;
		flex-direction: column;
		font-size: 13.5px;
		line-height: 1.6;
	}

	.turn {
		min-width: 0;
		padding: 8px 0 10px;
	}

	/* What the person said opens an exchange: a quiet band from edge to edge of the column. */
	.turn.you {
		margin: 10px calc(var(--gutter) * -0.5) 2px;
		padding: 7px calc(var(--gutter) * 0.5) 9px;
		border-top: 1px solid var(--line);
		background: var(--well);
	}

	.turn.you:first-child {
		margin-top: 0;
	}

	.meta {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 20px;
		margin-bottom: 1px;
	}

	.meta .num,
	.meta time {
		font-size: 11.5px;
		line-height: 16px;
		color: var(--muted);
	}

	.meta time {
		margin-left: auto;
	}

	.sep::before {
		content: '·';
		margin: 0 6px;
		color: var(--faint);
	}

	.mini {
		margin-left: auto;
		padding: 0 5px;
		border-radius: 3px;
		font: 400 11px/18px var(--rb-label);
		text-transform: uppercase;
		color: var(--muted);
		opacity: 0;
		transition: opacity var(--quick);
	}

	.mic {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		height: 26px;
		padding: 0 7px;
		border-radius: 3px;
		font-size: 12px;
		color: var(--muted);
	}

	.mic:hover:not(:disabled) {
		background: var(--hover);
		color: var(--ink);
	}

	/* Listening: the one red thing on the screen, so it is never left on by mistake. */
	.mic.live {
		color: var(--fail);
		box-shadow: inset 0 0 0 1px var(--fail);
	}

	.mini + .mini {
		margin-left: 0;
	}

	.turn:hover .mini,
	.mini.on,
	.mini:focus-visible {
		opacity: 1;
	}

	.mini:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.said {
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		font-weight: 500;
	}

	.waiting {
		display: flex;
		align-items: center;
		gap: 8px;
		font-size: 12px;
		color: var(--muted);
	}

	.waiting .clock {
		color: var(--ink);
	}

	.dot {
		width: 6px;
		height: 6px;
		background: var(--accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}

	.called {
		margin-top: 2px;
		font-size: 12px;
		color: var(--muted);
	}

	.called .fn {
		color: var(--accent);
	}

	.turn.offer {
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding-left: 10px;
		border-left: 2px solid var(--line);
		color: var(--muted);
	}

	.offer .row {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px;
		margin-top: 4px;
	}

	.problem {
		margin-top: 4px;
		font-size: 12.5px;
	}

	/* Rendered Markdown */
	.prose {
		max-width: 76ch;
		overflow-wrap: anywhere;
	}

	/* The answer being written ends in a cursor. */
	.prose.writing > :global(:last-child)::after {
		content: '';
		display: inline-block;
		width: 6px;
		height: 13px;
		margin-left: 3px;
		vertical-align: -2px;
		background: var(--accent);
	}

	.prose :global(:first-child) {
		margin-top: 0;
	}

	.prose :global(:last-child) {
		margin-bottom: 0;
	}

	.prose :global(p),
	.prose :global(ul),
	.prose :global(ol),
	.prose :global(blockquote),
	.prose :global(table),
	.prose :global(.code) {
		margin: 0 0 0.7em;
	}

	.prose :global(h1),
	.prose :global(h2),
	.prose :global(h3),
	.prose :global(h4) {
		margin: 1.2em 0 0.4em;
		font: 600 13px/1.3 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.prose :global(h1) {
		font-size: 14.5px;
	}

	.prose :global(ul),
	.prose :global(ol) {
		padding-left: 1.5em;
	}

	.prose :global(ul) {
		list-style: square;
	}

	.prose :global(ol) {
		list-style: decimal;
	}

	.prose :global(li + li) {
		margin-top: 0.15em;
	}

	.prose :global(li::marker) {
		color: var(--muted);
	}

	.prose :global(code) {
		padding: 0.08em 0.3em;
		border-radius: 3px;
		background: var(--board);
		font: 0.9em var(--rb-data);
	}

	.prose :global(.code) {
		max-width: none;
		border: 1px solid var(--line);
		border-radius: var(--r);
		overflow: hidden;
	}

	.prose :global(.code-head) {
		display: flex;
		align-items: center;
		justify-content: space-between;
		height: 24px;
		padding: 0 4px 0 10px;
		border-bottom: 1px solid var(--line);
		background: var(--well);
		font: 400 11px/1 var(--rb-label);
		text-transform: uppercase;
		color: var(--muted);
	}

	.prose :global(.code-head button) {
		height: 18px;
		padding: 0 6px;
		border-radius: 3px;
		font: inherit;
		text-transform: uppercase;
		color: var(--muted);
	}

	.prose :global(.code-head button:hover) {
		background: var(--hover);
		color: var(--ink);
	}

	.prose :global(pre) {
		margin: 0;
		padding: 9px 12px;
		overflow-x: auto;
		font: 12.5px/1.55 var(--rb-data);
		tab-size: 2;
	}

	.prose :global(pre code) {
		padding: 0;
		background: none;
		font: inherit;
	}

	.prose :global(blockquote) {
		padding-left: 10px;
		border-left: 2px solid var(--line);
		color: var(--muted);
	}

	.prose :global(table) {
		display: block;
		overflow-x: auto;
		border-collapse: collapse;
		font-size: 12.5px;
	}

	.prose :global(th),
	.prose :global(td) {
		padding: 4px 14px 4px 0;
		border-bottom: 1px solid var(--line);
		text-align: left;
	}

	.prose :global(th) {
		font: 400 11px/1.4 var(--rb-label);
		text-transform: uppercase;
		color: var(--muted);
	}

	.prose :global(hr) {
		border: 0;
		border-top: 1px solid var(--line);
		margin: 1em 0;
	}

	/* --- the composer, docked above the bay --- */
	.dock {
		flex: none;
		padding: 0 0 12px;
	}

	.composer {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 5px;
		background: var(--bg);
		transition: border-color var(--quick);
	}

	.composer:focus-within {
		border-color: var(--accent);
		box-shadow: 0 0 0 1px var(--accent);
	}

	.composer.off {
		background: var(--well);
	}

	textarea {
		width: 100%;
		min-width: 0;
		max-height: 200px;
		padding: 9px 11px 4px;
		border: 0;
		background: transparent;
		resize: none;
		font-size: 13.5px;
		line-height: 1.5;
		cursor: text;
	}

	textarea {
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
		align-items: center;
		gap: 8px;
		min-height: 30px;
		padding: 0 5px 5px 7px;
	}

	.grow {
		flex: 1;
	}

	.chips {
		display: inline-flex;
		gap: 1px;
		padding: 1px;
		border-radius: 4px;
		box-shadow: inset 0 0 0 1px var(--line);
	}

	.chips button {
		display: inline-flex;
		align-items: center;
		height: 20px;
		padding: 0 7px;
		border-radius: 3px;
		font: 400 11px/1 var(--rb-label);
		text-transform: uppercase;
		letter-spacing: 0.02em;
		color: var(--muted);
		transition:
			background-color var(--quick),
			color var(--quick);
	}

	.chips button:hover {
		color: var(--ink);
	}

	.chips button[aria-checked='true'] {
		background: var(--ink);
		color: var(--bg);
	}

	.chips button:disabled {
		opacity: 0.55;
	}

	.shape {
		height: 10px;
		max-width: 14px;
		border: 1.5px solid currentColor;
		border-radius: 1px;
	}

	.to {
		font-size: 11.5px;
		color: var(--muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.pick {
		padding: 2px 5px;
		margin-left: -5px;
		border-radius: 3px;
	}

	.pick:not(:disabled):hover {
		background: var(--hover);
		color: var(--ink);
	}

	.go {
		flex: none;
		display: inline-flex;
		align-items: center;
		gap: 6px;
		height: 22px;
		padding: 0 4px 0 9px;
		border-radius: 3px;
		background: var(--accent);
		color: var(--on-accent);
		font-size: 12px;
		font-weight: 600;
		transition:
			background-color var(--quick),
			opacity var(--quick);
	}

	.go kbd {
		box-shadow: none;
		background: color-mix(in srgb, var(--on-accent) 22%, transparent);
		color: inherit;
	}

	.go:hover {
		background: color-mix(in srgb, var(--accent) 86%, var(--ink));
	}

	.go:disabled {
		background: transparent;
		box-shadow: inset 0 0 0 1px var(--line);
		color: var(--faint);
	}

	.go:disabled kbd {
		background: none;
		box-shadow: inset 0 0 0 1px var(--line);
	}

	.go.stop {
		background: var(--ink);
		color: var(--bg);
	}

	.latest {
		position: absolute;
		right: var(--gutter);
		top: -30px;
		display: inline-flex;
		align-items: center;
		gap: 4px;
		height: 22px;
		padding: 0 8px 0 6px;
		border-radius: 3px;
		box-shadow: inset 0 0 0 1px var(--line);
		background: var(--bg);
		font-size: 12px;
		font-weight: 500;
		animation: appear 0.15s var(--ease) both;
	}

	/* The inline command menu grows upwards from the composer. */
	.slash {
		position: absolute;
		left: var(--gutter);
		bottom: calc(100% + 4px);
		z-index: 5;
		width: min(440px, calc(100% - var(--gutter) * 2));
		padding: 4px;
		border: 1px solid var(--line);
		border-radius: 5px;
		background: var(--bg);
		box-shadow: 0 10px 30px -12px var(--shade);
		animation: appear 0.12s var(--ease) both;
	}

	.slash li {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 26px;
		padding: 0 7px;
		border-radius: 3px;
	}

	.slash li[aria-selected='true'] {
		background: var(--accent-soft);
	}

	.cmd {
		flex: none;
		min-width: 62px;
		font-size: 12px;
		color: var(--ink);
	}

	.does {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12.5px;
		color: var(--muted);
	}

	/* --- the side pane (only drawn in a window of 1080px or more) --- */
	.chat.paned {
		grid-template-columns: minmax(0, 1fr) clamp(272px, calc(100vw - 960px), 600px);
	}

	/* Guided: the pane only lists chats, so it stays narrow. */
	.chat.paned.slim {
		grid-template-columns: minmax(0, 1fr) clamp(240px, calc(100vw - 1000px), 300px);
	}

	.pane {
		display: grid;
		grid-template-columns: repeat(auto-fit, minmax(240px, 1fr));
		align-content: start;
		gap: 4px 24px;
		padding: 8px 16px 16px;
		border-left: 1px solid var(--line);
		background: var(--bg);
	}

	.pane section {
		min-width: 0;
		padding-bottom: 10px;
	}

	.ptitle {
		margin: 2px 0 6px;
		font-weight: 500;
		overflow-wrap: anywhere;
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		overflow: hidden;
	}

	.figures div {
		display: flex;
		justify-content: space-between;
		gap: 12px;
		height: 22px;
		align-items: center;
		border-bottom: 1px solid var(--line-soft);
	}

	.figures dt {
		color: var(--muted);
		font-size: 12.5px;
	}

	.figures dd {
		font-size: 12px;
	}

	.chats li {
		position: relative;
		display: flex;
		align-items: center;
		margin: 0 -6px;
	}

	.chats .open {
		display: flex;
		align-items: center;
		gap: 8px;
		flex: 1;
		min-width: 0;
		height: 24px;
		padding: 0 6px;
		border-radius: 3px;
		font-size: 12.5px;
		color: var(--muted);
	}

	.chats .open:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.chats .open.current {
		background: var(--pressed);
		color: var(--ink);
	}

	/* Delete sits on the row's end, shown on hover or keyboard focus. */
	.chats .del {
		position: absolute;
		right: 2px;
		display: grid;
		place-items: center;
		width: 22px;
		height: 20px;
		border-radius: 3px;
		background: var(--bg);
		color: var(--muted);
		opacity: 0;
	}

	.chats li:hover .del,
	.chats .del:focus-visible {
		opacity: 1;
	}

	.chats .del:hover {
		color: var(--fail);
		background: var(--hover);
	}

	.chats .num {
		flex: none;
		font-size: 11px;
		color: var(--faint);
	}

	.quiet {
		margin-top: 6px;
		font-size: 12px;
		color: var(--muted);
	}

	.quiet.problem {
		color: var(--fail);
	}

	/* --- Guided: the plain first screen --- */
	.column.plain {
		max-width: 760px;
	}

	.first {
		padding: 28px 0 24px;
	}

	.first h1 {
		font-size: 19px;
		text-wrap: balance;
	}

	.lede {
		margin-top: 6px;
		max-width: 60ch;
		color: var(--muted);
	}

	.first .row {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 12px;
	}

	.starts {
		margin-top: 14px;
		border-top: 1px solid var(--line);
	}

	.starts li {
		border-bottom: 1px solid var(--line-soft);
	}

	.starts button {
		display: grid;
		grid-template-columns: 28px 224px minmax(0, 1fr) 14px;
		align-items: center;
		gap: 10px;
		width: 100%;
		min-height: 40px;
		padding: 6px 6px 6px 2px;
		text-align: left;
		transition: background-color var(--quick);
	}

	.starts button:hover {
		background: var(--hover);
	}

	.starts button:hover .st,
	.starts button:hover :global(svg) {
		color: var(--accent);
	}

	.starts button > :global(svg) {
		color: var(--faint);
	}

	.ico {
		display: grid;
		place-items: center;
		width: 28px;
		height: 28px;
		border-radius: var(--r);
		background: var(--well);
		box-shadow: inset 0 0 0 1px var(--line-soft);
		color: var(--ink);
	}

	.st {
		font: 600 13.5px/1.2 var(--rb-display);
		letter-spacing: -0.045em;
		white-space: nowrap;
	}

	.sd {
		min-width: 0;
		color: var(--muted);
		font-size: 12.5px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.check {
		margin-top: 22px;
	}

	.first .rule {
		margin-top: 22px;
	}

	.owned li {
		display: grid;
		grid-template-columns: 28px auto minmax(0, 1fr) auto;
		align-items: center;
		gap: 10px;
		min-height: 36px;
		border-bottom: 1px solid var(--line-soft);
	}

	.owned .ico {
		width: 24px;
		height: 24px;
		background: none;
		box-shadow: none;
		color: var(--muted);
	}

	.oname {
		font: 600 12.5px/1.2 var(--rb-display);
		letter-spacing: -0.045em;
		white-space: nowrap;
	}

	.oname:hover {
		color: var(--accent);
	}

	.odoes {
		min-width: 0;
		color: var(--muted);
		font-size: 12.5px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.addable {
		margin-top: 8px;
		font-size: 12.5px;
		color: var(--muted);
	}

	.addable .link + .link {
		margin-left: 6px;
	}

	/* --- an open chat's head: its name and Delete --- */
	.head {
		position: sticky;
		top: 0;
		z-index: 3;
		display: flex;
		align-items: center;
		gap: 8px;
		height: 36px;
		margin: 0 calc(var(--gutter) * -0.5);
		padding: 0 calc(var(--gutter) * 0.5);
		border-bottom: 1px solid var(--line-soft);
		background: var(--bg);
	}

	.head .title {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
		height: 26px;
		margin-left: -6px;
		padding: 0 6px;
		border-radius: var(--r);
		color: var(--ink);
	}

	.head .title :global(svg) {
		flex: none;
		color: var(--faint);
	}

	.head .title:hover {
		background: var(--hover);
	}

	.head .title:hover :global(svg) {
		color: var(--ink);
	}

	.tname {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 600 13px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.rename {
		flex: 1;
		min-width: 0;
		height: 26px;
		margin-left: -6px;
		padding: 0 6px;
		border: 0;
		border-radius: var(--r);
		box-shadow: inset 0 0 0 1px var(--accent);
		background: var(--bg);
		font: 600 13px/1 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.rename:focus-visible {
		outline: none;
	}

	.when {
		flex: none;
		font-size: 11.5px;
		color: var(--muted);
	}

	.head .tool {
		flex: none;
		display: grid;
		place-items: center;
		width: 28px;
		height: 26px;
		border-radius: var(--r);
		color: var(--muted);
	}

	.head .tool:hover {
		background: var(--hover);
		color: var(--fail);
	}

	.head .tool:disabled {
		opacity: 0.4;
	}

	.opening {
		padding: 16px 0;
	}

	.keys div {
		display: flex;
		align-items: center;
		gap: 10px;
		height: 22px;
	}

	.keys dt {
		flex: none;
		width: 48px;
	}

	.keys dd {
		font-size: 12.5px;
		color: var(--muted);
	}

	@media (max-width: 820px) {
		.route,
		.roster .dim,
		.roster .can {
			display: none;
		}

		.starts button {
			grid-template-columns: 28px minmax(0, 1fr) 14px;
		}

		.sd {
			grid-column: 2;
			white-space: normal;
		}

		.starts button > :global(svg) {
			grid-row: 1 / span 2;
			grid-column: 3;
		}

		.starts .ico {
			grid-row: 1 / span 2;
		}

		.owned li {
			grid-template-columns: 24px auto minmax(0, 1fr) auto;
		}
	}

	@media (max-width: 640px) {
		.odoes {
			display: none;
		}
	}
</style>
