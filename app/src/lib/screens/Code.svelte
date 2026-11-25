<script lang="ts">
	import { onMount, tick } from 'svelte';
	import { copyText, isTauri, openExternal, type AgentToolStep } from '#lib/api.ts';
	import { shownArguments } from '#lib/chattools.ts';
	import { code, isActive, modes, purposeOf } from '#lib/code.svelte.ts';
	import { connectors } from '#lib/connectors.svelte.ts';
	import { ago } from '#lib/format.ts';
	import { renderMarkdown } from '#lib/markdown.ts';
	import { skills } from '#lib/skills.svelte.ts';
	import { app } from '#lib/state.svelte.ts';
	import { isMac, machine, mod } from '#lib/words.ts';
	import Icon from '#lib/components/Icon.svelte';

	// Code: pick a folder, say what should be done, and watch a model on this computer do it one
	// step at a time. The same column as the chat: what you said on the right, the answer as
	// plain text, and between them one quiet line for each thing the model did. A change to a
	// file or a command stops at a card and waits for a yes.

	let draft = $state('');
	let typed = $state(code.folder);
	let input = $state<HTMLTextAreaElement>();
	let thread = $state<HTMLElement>();

	const task = $derived(code.task);
	const working = $derived(isActive(task));
	const model = $derived(app.chatPackages[0] ?? null);
	const folderName = $derived(code.folder.split(/[/\\]/).filter(Boolean).pop() ?? code.folder);
	const mode = $derived(modes.find((m) => m.id === code.mode) ?? modes[0]);
	const nameOf = (folder: string) => folder.split(/[/\\]/).filter(Boolean).pop() ?? folder;
	const others = $derived(code.recent.filter((f) => f !== code.folder));
	const plan = $derived(task?.plan ?? []);
	const planDone = $derived(plan.filter((s) => s.status === 'done').length);

	onMount(() => {
		code.enter();
		return () => code.leave();
	});

	// Follow the work: the newest step stays in view.
	$effect(() => {
		void task?.steps.length;
		void task?.pending;
		void task?.state;
		void code.writing;
		tick().then(() => thread?.scrollTo({ top: thread.scrollHeight }));
	});

	function grow() {
		if (!input) return;
		input.style.height = 'auto';
		input.style.height = `${Math.min(input.scrollHeight, 200)}px`;
	}

	async function send() {
		if (working || !draft.trim()) return;
		const text = draft;
		if (await code.send(text, model?.id)) {
			draft = '';
			await tick();
			grow();
		}
	}

	// Enter sends and Shift+Enter makes a new line. With Cmd or Ctrl held, Enter belongs to the
	// card that waits for an answer (see `onkey`).
	function onkeydown(event: KeyboardEvent) {
		if (event.key === 'Enter' && !event.shiftKey && !event.isComposing && !event.metaKey && !event.ctrlKey) {
			event.preventDefault();
			send();
		}
	}

	/** Esc asks before it stops the work: shown as a line above the box to type in. */
	let stopping = $state(false);
	$effect(() => {
		if (!working) stopping = false;
	});

	// Keys that work anywhere on the screen: Cmd or Ctrl with Enter allows the step that waits,
	// Esc asks whether to stop (and a second Esc takes the question back).
	function onkey(event: KeyboardEvent) {
		if (event.isComposing || !task) return;
		if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
			if (task.pending && !code.busy) {
				event.preventDefault();
				code.answer(true);
			} else if (!working && document.activeElement === input) {
				event.preventDefault();
				send();
			}
		} else if (event.key === 'Escape' && working) {
			event.preventDefault();
			stopping = !stopping;
		}
	}

	async function stop() {
		stopping = false;
		await code.stop();
	}

	// --- what the newest request changed -------------------------------------------------------

	interface Changed {
		path: string;
		plus: number;
		minus: number;
		created: boolean;
		diffs: string[];
	}

	/** How many messages the person sent: the number of the request being looked at. */
	const requests = $derived(task?.steps.filter((s) => s.kind === 'user').length ?? 0);

	/** The files the newest request changed, with the lines that came and went. */
	const changed = $derived.by((): Changed[] => {
		if (!task) return [];
		const from = task.steps.findLastIndex((s) => s.kind === 'user');
		const files = new Map<string, Changed>();
		for (const step of task.steps.slice(from + 1)) {
			if (step.kind !== 'tool' || step.status !== 'done' || !step.diff || (step.name !== 'edit_file' && step.name !== 'write_file' && step.name !== 'append_file')) continue;
			const path = text(step, 'path');
			const file = files.get(path) ?? { path, plus: 0, minus: 0, created: false, diffs: [] };
			for (const l of step.diff.split('\n')) {
				if (l.startsWith('+') && !l.startsWith('+++')) file.plus++;
				else if (l.startsWith('-') && !l.startsWith('---')) file.minus++;
			}
			if (step.diff.startsWith('--- /dev/null') && file.diffs.length === 0) file.created = true;
			file.diffs.push(step.diff);
			files.set(path, file);
		}
		return [...files.values()];
	});

	/** The newest request that can still be taken back, if there is one. */
	const undoable = $derived((task?.undo ?? []).findLast((u) => !u.undone && (u.kind === 'git' || (u.files?.length ?? 0) > 0)) ?? null);
	const takenBack = $derived((task?.undo ?? []).some((u) => u.undone && u.request === requests));

	const text = (step: AgentToolStep, key: string) => (typeof step.arguments[key] === 'string' ? (step.arguments[key] as string) : '');
	const short = (value: string, max = 64) => (value.length > max ? `${value.slice(0, max - 1)}…` : value);

	/** What a step did, as one plain line. */
	function line(step: AgentToolStep): string {
		const path = text(step, 'path');
		// Something that waits is wanted; something that never ran was only asked for.
		const wants = step.status === 'waiting';
		const asked = step.status === 'denied' || (step.status === 'stopped' && !step.result);
		const verb = (did: string, bare: string) => (wants ? `Wants to ${bare}` : asked ? `Asked to ${bare}` : did);
		switch (step.name) {
			case 'list_files':
				return `Looked in ${!path || path === '.' ? 'the folder' : path}`;
			case 'read_file':
				return `Read ${path}`;
			case 'search':
				return `Searched for “${short(text(step, 'pattern'), 48)}”`;
			case 'edit_file':
				return `${verb('Changed', 'change')} ${path}`;
			case 'write_file':
				return `${verb('Wrote', 'write')} ${path}`;
			case 'append_file':
				return `${verb('Added to', 'add to')} ${path}`;
			case 'run_command':
				return `${verb('Ran', 'run')} ${short(text(step, 'command'))}`;
			case 'web_search':
				return `Searched the web for “${short(text(step, 'query'), 48)}”`;
			case 'fetch_url':
				return `Opened ${short(text(step, 'url').replace(/^https?:\/\//, ''), 56)}`;
			case 'outline':
				return `Looked at what is in ${!path || path === '.' ? 'the folder' : path}`;
			case 'update_plan':
				return 'Updated the plan';
			case 'use_skill':
				return `Read the skill ${text(step, 'name')}`;
			case 'read_skill_file':
				return `Read ${short(text(step, 'path'), 40)} of the skill ${text(step, 'name')}`;
			case 'find_tool':
				return `Looked for a tool: “${short(text(step, 'query'), 48)}”`;
			case 'call_tool':
				return `${step.status === 'failed' ? 'Tried to use' : verb('Used', 'use')} ${short(text(step, 'tool'), 40)} from the connector ${short(text(step, 'connector'), 30)}`;
			default:
				return step.name;
		}
	}

	const tails: Record<AgentToolStep['status'], string> = {
		running: 'working',
		waiting: 'waiting for you',
		done: '',
		failed: 'did not work',
		denied: 'not allowed',
		stopped: 'stopped'
	};

	const states: Record<string, string> = {
		running: 'Working',
		waiting: 'Waiting for you',
		done: 'Done',
		failed: 'Did not finish',
		stopped: 'Stopped',
		interrupted: 'Cut off'
	};

	/** The arguments as the model sent them, without the long ones the diff already shows. */
	function shown(step: AgentToolStep): string {
		const out: Record<string, unknown> = {};
		for (const [key, value] of Object.entries(step.arguments)) {
			out[key] = step.diff && typeof value === 'string' && value.length > 120 ? `${value.slice(0, 120)}…` : value;
		}
		return JSON.stringify(out, null, 2);
	}

	const diffLines = (diff: string) =>
		diff.split('\n').map((l) => ({ text: l, kind: l.startsWith('+++') || l.startsWith('---') ? 'meta' : l.startsWith('@@') ? 'meta' : l.startsWith('+') ? 'plus' : l.startsWith('-') ? 'minus' : '' }));

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

	/** The step a waiting connector tool belongs to: it holds the tool and the arguments. */
	const asked = $derived(task?.pending?.tool === 'call_tool' ? task.steps.find((s) => s.kind === 'tool' && s.call === task.pending?.call) : undefined);

	const ideas = ['Summarise this folder', 'Why does the test fail?', 'Write a README'];
</script>

<svelte:window onkeydown={onkey} />

{#snippet diffView(diff: string)}
	<pre class="diff selectable">{#each diffLines(diff) as l, i (i)}<span class={l.kind}>{l.text}
</span>{/each}</pre>
{/snippet}

{#snippet folders(list: string[])}
	<ul class="recent">
		{#each list as f (f)}
			<li>
				<button class="task" onclick={() => code.setFolder(f)} title="Open {f}">
					<Icon name="folder" size={14} />
					<span class="name">{nameOf(f)}</span>
					<span class="when path">{f}</span>
				</button>
				<button class="del" onclick={() => code.forget(f)} title="Take this folder off the list. Nothing is deleted." aria-label="Take {nameOf(f)} off the list"><Icon name="trash" size={13} /></button>
			</li>
		{/each}
	</ul>
{/snippet}

{#snippet composer(placeholder: string, explain: boolean)}
	<div class="composer" class:off={!model}>
		<textarea bind:this={input} bind:value={draft} oninput={grow} {onkeydown} rows="1" spellcheck="false" disabled={!model} {placeholder} aria-label="What should be done"></textarea>
		<div class="tools">
			<div class="chips" role="radiogroup" aria-label="How much it may do">
				{#each modes as m (m.id)}
					<button type="button" role="radio" aria-checked={code.mode === m.id} onclick={() => code.setMode(m.id)} title={m.does}>{m.name}</button>
				{/each}
			</div>
			<label class="switch" title="Lets the model search the web and open pages">
				<input type="checkbox" checked={code.web} onchange={(e) => code.setWeb(e.currentTarget.checked)} />
				<span>Allow web search</span>
			</label>
			{#if connectors.list.length > 0}
				<label class="switch" title="Lets the model use the tools of your running connectors. It asks you each time.">
					<input type="checkbox" checked={code.connectors} onchange={(e) => code.setConnectors(e.currentTarget.checked)} />
					<span>Use connectors</span>
				</label>
			{/if}
			<span class="grow"></span>
			{#if working}
				<button class="go stop" onclick={stop} title="Stops the work where it is (Esc)">Stop</button>
			{:else}
				<button class="go" onclick={send} disabled={!draft.trim() || !model || code.busy}>Send</button>
			{/if}
		</div>
	</div>
	{#if explain}
		<dl class="modes">
			{#each modes as m (m.id)}
				<div class:on={code.mode === m.id}>
					<dt>{m.name}</dt>
					<dd>{m.does}</dd>
				</div>
			{/each}
		</dl>
	{/if}
	<p class="fine">
		{#if !explain}{mode.does}{/if}
		{#if code.web}
			Web search is on: the search words go to DuckDuckGo and the pages the model opens are fetched from the internet. Your files are not sent.
		{:else}
			Everything stays on this {machine}. Web search is off; if you switch it on, the search words go to DuckDuckGo.
		{/if}
		{#if code.connectors && connectors.list.length > 0}
			Connectors are on: {connectors.usable.length === 0 ? 'none is running, so start one under Connectors first' : 'the model may ask to use a tool of a running connector, and you are asked each time'}.
		{/if}
	</p>
{/snippet}

<section class="screen code" aria-label="Code">
	{#if !code.available}
		<div class="thread scroll">
			<div class="column home">
				<div class="hello">
					<h1>Code needs a newer service</h1>
					<p class="lede">The background service that is running does not know Code yet. Quit Rigbrew and open it again.</p>
				</div>
			</div>
		</div>
	{:else if !code.folder}
		<div class="thread scroll">
			<div class="column home">
				<div class="hello">
					<h1>Work on a folder</h1>
					<p class="lede">Choose a project folder and say what you want done. A model on this {machine} reads the files, does the work step by step, and asks before it changes anything.</p>
					{#if isTauri}
						<button class="pick" onclick={() => code.choose()}><Icon name="folder" size={16} />Choose a folder</button>
					{:else}
						<form class="path" onsubmit={(e) => (e.preventDefault(), code.setFolder(typed))}>
							<input type="text" bind:value={typed} placeholder="/path/to/your/project" spellcheck="false" aria-label="Folder path" />
							<button class="pick" type="submit" disabled={!typed.trim()}>Use this folder</button>
						</form>
					{/if}
					{#if code.problem}<p class="problem selectable">{code.problem}</p>{/if}
				</div>
				{#if code.recent.length}
					<div class="again">
						<h2>Recent folders</h2>
						{@render folders(code.recent)}
					</div>
				{/if}
			</div>
		</div>
	{:else}
		<header class="head">
			<div class="column bar">
				{#if task}
					<button class="back" onclick={() => code.close()} title="Back to the tasks of this folder"><Icon name="back" size={15} />Tasks</button>
					<span class="title" title={task.title}>{task.title}</span>
					<span class="state" class:live={working}>{states[task.state] ?? task.state}</span>
				{:else}
					<span class="where" title={code.folder}><Icon name="folder" size={15} /><span>{folderName}</span></span>
					<span class="grow"></span>
					{#if isTauri}
						<button class="back" onclick={() => code.choose()}>Change folder</button>
					{:else}
						<button class="back" onclick={() => ((typed = code.folder), (code.folder = ''))}>Change folder</button>
					{/if}
				{/if}
			</div>
		</header>

		{#if task && plan.length}
			<div class="column">
				<details class="plan" open>
					<summary>Plan <span class="count">{planDone} of {plan.length} done</span></summary>
					<ol>
						{#each plan as s, i (i)}
							<li class={s.status} aria-current={s.status === 'doing' ? 'step' : undefined}>
								<span class="box" aria-hidden="true">{#if s.status === 'done'}<Icon name="check" size={11} />{/if}</span>
								<span class="what">{s.text}</span>
								<span class="sr">{s.status === 'done' ? 'done' : s.status === 'doing' ? 'in progress' : 'to do'}</span>
							</li>
						{/each}
					</ol>
				</details>
			</div>
		{/if}

		{#if task}
			<div class="thread scroll" bind:this={thread}>
				<div class="column">
					<ol class="turns">
						{#each task.steps as step, i (i)}
							{#if step.kind === 'user'}
								<li class="turn you"><p class="said selectable">{step.text}</p></li>
							{:else if step.kind === 'model'}
								<li class="turn model">
									<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
									<div class="prose selectable" onclick={onanswerclick}>{@html renderMarkdown(step.text)}</div>
								</li>
							{:else if step.kind === 'note'}
								<li class="note">{step.text}</li>
							{:else if step.kind === 'tool'}
								<li class="step {step.status}">
									<details>
										<summary>
											<span class="mark" aria-hidden="true"></span>
											<span class="did">{line(step)}</span>
											{#if tails[step.status]}<span class="tail">{tails[step.status]}</span>{/if}
										</summary>
										<div class="inside">
											{#if step.diff}{@render diffView(step.diff)}{/if}
											<p class="cap">Asked for</p>
											<pre class="raw selectable">{step.name} {shown(step)}</pre>
											{#if step.result}
												<p class="cap">Result</p>
												<pre class="raw selectable">{step.result}</pre>
											{/if}
										</div>
									</details>
								</li>
							{/if}
						{/each}
						{#if working && code.writing}
							<li class="turn model writing">
								<div class="prose selectable">{@html renderMarkdown(code.writing)}</div>
							</li>
						{/if}
					</ol>

					{#if task.pending}
						{@const p = task.pending}
						<div class="ask" role="group" aria-label="Waiting for your answer">
							{#if p.command}
								<h2>Run this command?</h2>
								<pre class="raw selectable">{p.command}</pre>
								{#if purposeOf(p.command)}<p class="purpose">{purposeOf(p.command)}</p>{/if}
								{#if isMac}
									<p class="why">It runs in {folderName}. It can read what you can, change files only inside this folder{code.web ? '' : ', and it has no internet'}. Allow it only if you know what it does.</p>
								{:else}
									<p class="why">It runs in {folderName} with your own rights, so it can do anything you can do in a terminal. Allow it only if you know what it does.</p>
								{/if}
							{:else if p.tool === 'call_tool'}
								<h2>Use {asked?.kind === 'tool' ? String(asked.arguments.tool ?? 'a tool') : 'a tool'} from the connector {p.path}?</h2>
								<p class="cap">With these arguments</p>
								<pre class="raw selectable">{shownArguments(asked?.kind === 'tool' ? asked.arguments.arguments : undefined)}</pre>
								<p class="why">This tool belongs to the connector, not to Rigbrew. It works outside this folder and may reach other files, programs or the internet. Allow it only if you know what it does.</p>
							{:else}
								<h2>{p.tool === 'write_file' ? 'Write' : p.tool === 'append_file' ? 'Add to' : 'Change'} {p.path}?</h2>
								{#if p.diff}{@render diffView(p.diff)}{/if}
							{/if}
							<div class="answers">
								<button class="go" onclick={() => code.answer(true)} disabled={code.busy} title="Allow ({mod}Enter)">Allow</button>
								<button class="plainbtn" onclick={() => code.answer(false)} disabled={code.busy}>Deny</button>
								<span class="grow"></span>
								<button
									class="plainbtn"
									onclick={() => code.answer(true, true)}
									disabled={code.busy}
									title={p.command ? 'Runs this exact command without asking again in this task' : p.tool === 'call_tool' ? 'Uses this tool without asking again in this task' : 'Applies every file change in this task without asking'}
								>
									{p.command ? 'Always allow this command' : p.tool === 'call_tool' ? 'Always allow this tool' : 'Always allow file changes'}
								</button>
							</div>
							<p class="keys">{mod}Enter allows it.</p>
						</div>
					{:else if task.state === 'running'}
						<p class="working"><span class="dot"></span>{code.writing ? 'Writing' : 'Working'}</p>
					{/if}

					{#if !working && !takenBack && (changed.length || (undoable && undoable.request === requests))}
						<div class="changed" role="group" aria-label="What changed">
							<h2>What changed</h2>
							{#if changed.length}
								<ul>
									{#each changed as file (file.path)}
										<li>
											<details>
												<summary>
													<span class="file">{file.path}</span>
													{#if file.created}<span class="tag">new</span>{/if}
													<span class="plus" aria-label="{file.plus} lines added">+{file.plus}</span>
													<span class="minus" aria-label="{file.minus} lines removed">−{file.minus}</span>
												</summary>
												{#each file.diffs as diff, i (i)}{@render diffView(diff)}{/each}
											</details>
										</li>
									{/each}
								</ul>
							{:else}
								<p class="fine">No file was changed with the model's own tools. A command ran, and it may have changed files.</p>
							{/if}
							{#if undoable && undoable.request === requests}
								<div class="answers">
									<button class="plainbtn" onclick={() => code.undo()} disabled={code.busy} title="Puts the files back as they were before this request">Undo this request</button>
									<span class="fine">
										{#if undoable.kind === 'git'}
											Every file goes back to how it was before this request. Files that git ignores stay as they are.
										{:else if undoable.commands}
											The files above go back to how they were. What commands changed cannot be undone, because this folder is not a git repository.
										{:else}
											The files above go back to how they were before this request.
										{/if}
									</span>
								</div>
							{/if}
						</div>
					{:else if !working && undoable && undoable.request !== requests}
						<div class="answers earlier">
							<button class="plainbtn" onclick={() => code.undo()} disabled={code.busy}>Undo the earlier changes</button>
							<span class="fine">Takes back what request {undoable.request} of this task changed.</span>
						</div>
					{/if}

					{#if task.error}<p class="ended selectable" class:bad={task.state === 'failed'}>{task.error}</p>
					{:else if task.state === 'stopped'}<p class="ended">Stopped. Nothing more will be changed. Send a message to go on.</p>{/if}
					{#if code.problem}<p class="problem selectable">{code.problem}</p>{/if}
				</div>
			</div>
			<div class="dock">
				<div class="column">
					{#if stopping}
						<div class="stopask" role="alertdialog" aria-label="Stop the work?">
							<span>Stop the work? What is already changed stays changed.</span>
							<span class="grow"></span>
							<!-- svelte-ignore a11y_autofocus -->
							<button class="plainbtn" onclick={stop} autofocus>Stop</button>
							<button class="plainbtn" onclick={() => (stopping = false)}>Keep going</button>
						</div>
					{/if}
					{@render composer(task.pending ? 'Answer the question above first' : working ? 'It is working. Press Esc to stop it.' : 'Ask for the next thing', false)}
				</div>
			</div>
		{:else}
			<div class="thread scroll">
				<div class="column start">
					<h1>What should be done in {folderName}?</h1>
					{#if !model}
						<p class="lede">Code needs a chat model. <button class="link" onclick={() => app.go('models')}>Install one under Models</button>, then come back.</p>
					{/if}
					{@render composer('Say it in plain words', true)}
					{#if code.problem}<p class="problem selectable">{code.problem}</p>{/if}

					{#if code.tasks.length === 0}
						<ul class="ideas">
							{#each ideas as idea (idea)}
								<li><button onclick={() => ((draft = idea), input?.focus())} disabled={!model}>{idea}</button></li>
							{/each}
						</ul>
					{:else}
						<h2>Tasks in this folder</h2>
						<ul class="tasks">
							{#each code.tasks as t (t.id)}
								<li>
									<button class="task" onclick={() => code.open(t.id)}>
										<span class="name">{t.title}</span>
										<span class="state" class:live={isActive(t)}>{states[t.state] ?? t.state}</span>
										<span class="when">{ago(t.updated_ms)}</span>
									</button>
									<button class="del" onclick={() => code.remove(t.id)} title="Remove this task from the list" aria-label="Remove task “{t.title}”"><Icon name="trash" size={13} /></button>
								</li>
							{/each}
						</ul>
					{/if}

					{#if others.length}
						<h2>Recent folders</h2>
						{@render folders(others)}
					{/if}

					<details class="skills">
						<summary>Skills <span class="count">{code.skills.length}</span></summary>
						<p class="lede">A skill is a short set of instructions for one kind of work. The model reads one when it fits what you asked.</p>
						<ul>
							{#each code.skills as s (s.name)}
								<li>
									<span class="name">{s.name}</span>
									<span class="from">{s.source === 'built-in' ? 'comes with Rigbrew' : s.source === 'folder' ? 'from this folder' : 'yours'}</span>
									<span class="does">{s.description}</span>
								</li>
							{/each}
						</ul>
						<p class="lede selectable">
							<button class="link" onclick={() => skills.open('code', code.folder)}>Manage skills</button> to read, switch off, write or import one. A project can carry its own in .rigbrew/skills.
						</p>
					</details>
				</div>
			</div>
		{/if}
	{/if}
</section>

<style>
	.code {
		font-family: var(--rb-text);
	}

	.column {
		width: 100%;
		max-width: 780px;
		margin: 0 auto;
		padding: 0 var(--gutter);
	}

	.thread {
		flex: 1;
		min-height: 0;
		padding-bottom: 28px;
	}

	.thread > .column.home {
		min-height: 100%;
		display: grid;
		align-content: center;
	}

	.grow {
		flex: 1;
	}

	/* --- nothing chosen yet --- */
	.hello {
		display: grid;
		justify-items: center;
		gap: 18px;
		padding: 24px 0 8px;
		text-align: center;
	}

	h1 {
		font: 600 clamp(22px, 2.6vw, 28px) / 1.2 var(--rb-text);
		letter-spacing: -0.02em;
	}

	.lede {
		max-width: 52ch;
		color: var(--muted);
		font-size: 15px;
		line-height: 1.5;
	}

	.start .skills .lede {
		max-width: none;
		justify-self: start;
		text-align: left;
		font-size: 13px;
		line-height: 1.45;
		overflow-wrap: anywhere;
	}

	.pick {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		height: 36px;
		padding: 0 16px;
		border-radius: 999px;
		background: var(--ink);
		color: var(--bg);
		font: 600 14px/1 var(--rb-text);
	}

	.pick:disabled {
		opacity: 0.3;
	}

	.path {
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: 8px;
		width: 100%;
	}

	.path input {
		flex: 1;
		min-width: 240px;
		max-width: 460px;
		height: 36px;
		padding: 0 14px;
		border: 1px solid var(--line);
		border-radius: 999px;
		background: var(--bg);
		font: 13px var(--rb-data);
		cursor: text;
	}

	/* --- the line on top: where, or which task --- */
	.head {
		flex: none;
		padding: 0 0 10px;
	}

	.bar {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 30px;
	}

	.where {
		display: inline-flex;
		align-items: center;
		gap: 8px;
		min-width: 0;
		font: 600 14px/1.3 var(--rb-text);
		color: var(--muted);
	}

	.where span,
	.title {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.title {
		flex: 1;
		font: 600 14px/1.3 var(--rb-text);
		color: var(--muted);
	}

	.back {
		flex: none;
		display: inline-flex;
		align-items: center;
		gap: 4px;
		height: 28px;
		padding: 0 10px;
		border-radius: 999px;
		font-size: 13px;
		color: var(--muted);
	}

	.back:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.state {
		flex: none;
		font-size: 12.5px;
		color: var(--faint);
	}

	.state.live {
		color: var(--accent);
	}

	/* --- the plan: a small checklist that stays above the work --- */
	.plan {
		flex: none;
		margin: 0 0 10px;
		padding: 8px 12px;
		border: 1px solid var(--line-soft);
		border-radius: 12px;
		font-size: 13.5px;
	}

	.plan summary {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 22px;
		cursor: pointer;
		font: 500 12.5px/1 var(--rb-text);
		color: var(--muted);
		list-style: none;
	}

	.plan summary::-webkit-details-marker {
		display: none;
	}

	.plan .count {
		color: var(--faint);
		font-weight: 400;
	}

	.plan ol {
		margin: 6px 0 2px;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 5px;
		max-height: 30vh;
		overflow: auto;
	}

	.plan li {
		display: flex;
		align-items: baseline;
		gap: 9px;
		color: var(--muted);
		line-height: 1.4;
	}

	.plan .box {
		flex: none;
		display: grid;
		place-items: center;
		width: 14px;
		height: 14px;
		border-radius: 4px;
		box-shadow: inset 0 0 0 1.5px var(--line);
		transform: translateY(2px);
	}

	.plan li.done {
		color: var(--faint);
	}

	.plan li.done .box {
		background: var(--fit-soft);
		box-shadow: none;
		color: var(--fit);
	}

	.plan li.doing {
		color: var(--ink);
	}

	.plan li.doing .box {
		box-shadow: inset 0 0 0 1.5px var(--accent);
	}

	.plan li.doing .box::after {
		content: '';
		width: 6px;
		height: 6px;
		border-radius: 2px;
		background: var(--accent);
	}

	.plan .what {
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.sr {
		position: absolute;
		width: 1px;
		height: 1px;
		overflow: hidden;
		clip-path: inset(50%);
		white-space: nowrap;
	}

	/* --- the start page of a folder --- */
	.start {
		display: grid;
		gap: 16px;
		padding-top: clamp(16px, 9vh, 96px);
	}

	.start h1 {
		text-align: center;
		margin-bottom: 6px;
		overflow-wrap: anywhere;
	}

	.start .lede {
		justify-self: center;
		text-align: center;
	}

	.start h2 {
		margin-top: 14px;
		padding: 0 4px;
		font: 500 12px/1 var(--rb-text);
		color: var(--faint);
	}

	.ideas {
		margin: 0;
		padding: 0;
		list-style: none;
		display: flex;
		flex-wrap: wrap;
		justify-content: center;
		gap: 8px;
	}

	.ideas button {
		height: 36px;
		padding: 0 14px;
		border: 1px solid var(--line);
		border-radius: 999px;
		font-size: 14px;
		color: var(--muted);
	}

	.ideas button:hover:not(:disabled) {
		background: var(--hover);
		color: var(--ink);
	}

	.tasks,
	.recent {
		margin: -8px 0 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 1px;
	}

	.tasks li,
	.recent li {
		position: relative;
	}

	.recent .task {
		align-items: center;
		color: var(--ink);
	}

	.recent .task .name {
		flex: none;
		max-width: 50%;
	}

	.recent .task .path {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--rb-data);
		font-size: 12px;
	}

	.again {
		display: grid;
		gap: 14px;
		width: min(520px, 100%);
		margin: 22px auto 0;
		text-align: left;
	}

	.again h2 {
		padding: 0 4px;
		font: 500 12px/1 var(--rb-text);
		color: var(--faint);
	}

	.task {
		display: flex;
		align-items: baseline;
		gap: 12px;
		width: 100%;
		min-height: 36px;
		padding: 8px 36px 8px 10px;
		border-radius: 8px;
		text-align: left;
		font-size: 14px;
	}

	.task:hover {
		background: var(--hover);
	}

	.task .name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.task .when {
		flex: none;
		font-size: 12.5px;
		color: var(--faint);
	}

	.del {
		position: absolute;
		right: 6px;
		top: 6px;
		width: 24px;
		height: 24px;
		display: none;
		place-items: center;
		border-radius: 6px;
		color: var(--muted);
	}

	.tasks li:hover .del,
	.recent li:hover .del,
	.del:focus-visible {
		display: grid;
	}

	.del:hover {
		background: var(--pressed);
		color: var(--fail);
	}

	.skills {
		margin-top: 10px;
		padding: 0 4px;
		font-size: 13px;
		color: var(--muted);
	}

	.skills summary {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		height: 28px;
		cursor: pointer;
		font-size: 12.5px;
		color: var(--faint);
		list-style: none;
	}

	.skills summary::-webkit-details-marker {
		display: none;
	}

	.skills summary::before {
		content: '›';
		display: inline-block;
		transition: transform var(--quick);
	}

	.skills[open] summary::before {
		transform: rotate(90deg);
	}

	.skills summary:hover {
		color: var(--ink);
	}

	.skills ul {
		margin: 10px 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 10px;
	}

	.skills li {
		display: grid;
		grid-template-columns: auto 1fr;
		gap: 2px 10px;
		align-items: baseline;
	}

	.skills .name {
		font: 500 13px var(--rb-data);
		color: var(--ink);
	}

	.skills .from {
		font-size: 12px;
		color: var(--faint);
	}

	.skills .does {
		grid-column: 1 / -1;
		line-height: 1.45;
	}

	/* --- the work, step by step --- */
	.turns {
		margin: 0;
		padding: 8px 0 0;
		list-style: none;
		display: grid;
		/* A long command in one line is cut with an ellipsis; it never widens the column. */
		grid-template-columns: minmax(0, 1fr);
		gap: 14px;
	}

	.turn.you {
		justify-self: end;
		max-width: 78%;
		padding: 10px 14px;
		border-radius: 18px;
		background: var(--well);
	}

	.said {
		font: 400 15px/1.5 var(--rb-text);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.note,
	.ended {
		font-size: 13px;
		color: var(--faint);
	}

	.ended {
		margin-top: 14px;
	}

	.ended.bad {
		color: var(--fail);
	}

	/* One quiet line for each thing the model did; it opens to show what was asked and what came back. */
	.step {
		margin: -6px 0;
	}

	.step summary {
		display: flex;
		align-items: center;
		gap: 9px;
		min-height: 26px;
		padding: 0 6px;
		margin: 0 -6px;
		border-radius: 6px;
		cursor: pointer;
		font-size: 13.5px;
		color: var(--muted);
		list-style: none;
	}

	.step summary::-webkit-details-marker {
		display: none;
	}

	.step summary:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.mark {
		flex: none;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: color-mix(in srgb, var(--muted) 45%, transparent);
	}

	.step.running .mark,
	.step.waiting .mark {
		background: var(--accent);
		animation: blink 1.2s steps(2, jump-none) infinite;
	}

	.step.failed .mark,
	.step.denied .mark {
		background: var(--warn);
	}

	.did {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.tail {
		flex: none;
		font-size: 12px;
		color: var(--faint);
	}

	.inside {
		display: grid;
		gap: 6px;
		margin: 6px 0 10px 15px;
		padding-left: 12px;
		border-left: 1px solid var(--line-soft);
	}

	.cap {
		font-size: 12px;
		color: var(--faint);
	}

	.raw,
	.diff {
		margin: 0;
		max-height: 320px;
		padding: 9px 12px;
		overflow: auto;
		border-radius: 8px;
		background: var(--well);
		font: 12.5px/1.55 var(--rb-data);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		tab-size: 2;
	}

	.diff {
		white-space: pre;
		overflow-wrap: normal;
	}

	.diff span {
		display: block;
		min-height: 1.55em;
	}

	.diff .meta {
		color: var(--faint);
	}

	.diff .plus {
		background: var(--fit-soft);
		color: var(--fit);
	}

	.diff .minus {
		background: var(--fail-soft);
		color: var(--fail);
	}

	/* --- a step that waits for a yes --- */
	.ask {
		display: grid;
		gap: 10px;
		margin-top: 16px;
		padding: 14px 16px;
		border: 1px solid var(--line);
		border-radius: 14px;
		background: var(--bg);
		box-shadow: 0 1px 2px color-mix(in srgb, var(--ink) 6%, transparent), 0 8px 24px -12px color-mix(in srgb, var(--ink) 18%, transparent);
		animation: appear 0.15s var(--ease) both;
	}

	.ask h2 {
		font: 600 15px/1.3 var(--rb-text);
		overflow-wrap: anywhere;
	}

	.why {
		font-size: 13px;
		line-height: 1.45;
		color: var(--muted);
	}

	.answers {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
	}

	.plainbtn {
		height: 32px;
		padding: 0 14px;
		border-radius: 999px;
		box-shadow: inset 0 0 0 1px var(--line);
		font: 500 13px/1 var(--rb-text);
	}

	.plainbtn:hover {
		background: var(--hover);
	}

	.plainbtn:disabled {
		opacity: 0.4;
	}

	.purpose,
	.keys {
		font-size: 13px;
		line-height: 1.45;
		color: var(--muted);
	}

	.keys {
		font-size: 12px;
		color: var(--faint);
	}

	/* --- what the request changed, and the way back --- */
	.changed {
		display: grid;
		gap: 8px;
		margin-top: 18px;
		padding: 12px 14px;
		border: 1px solid var(--line-soft);
		border-radius: 12px;
	}

	.changed h2 {
		font: 500 12.5px/1 var(--rb-text);
		color: var(--muted);
	}

	.changed ul {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 1px;
	}

	.changed summary {
		display: flex;
		align-items: baseline;
		gap: 10px;
		min-height: 28px;
		padding: 4px 6px;
		margin: 0 -6px;
		border-radius: 6px;
		cursor: pointer;
		font-size: 13.5px;
		list-style: none;
	}

	.changed summary::-webkit-details-marker {
		display: none;
	}

	.changed summary:hover {
		background: var(--hover);
	}

	.changed .file {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-family: var(--rb-data);
		font-size: 12.5px;
	}

	.changed .tag {
		flex: none;
		font-size: 12px;
		color: var(--faint);
	}

	.changed .plus,
	.changed .minus {
		flex: none;
		font: 12.5px var(--rb-data);
	}

	.changed .plus {
		margin-left: auto;
		color: var(--fit);
	}

	.changed .minus {
		color: var(--fail);
	}

	.changed .diff {
		margin: 4px 0 8px;
	}

	.changed .answers,
	.earlier {
		flex-wrap: nowrap;
		align-items: center;
		gap: 12px;
		margin-top: 4px;
	}

	.earlier {
		margin-top: 16px;
	}

	.changed .answers .plainbtn,
	.earlier .plainbtn {
		flex: none;
	}

	.changed .fine,
	.earlier .fine {
		margin: 0;
		padding: 0;
	}

	.stopask {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		margin-bottom: 8px;
		padding: 8px 8px 8px 14px;
		border: 1px solid var(--line);
		border-radius: 14px;
		background: var(--bg);
		font-size: 13.5px;
		animation: appear 0.15s var(--ease) both;
	}

	/* --- the three modes, each in one line --- */
	.modes {
		display: grid;
		gap: 3px;
		margin: 10px 0 0;
		padding: 0 10px;
		font-size: 12.5px;
		line-height: 1.45;
		color: var(--faint);
	}

	.modes div {
		display: grid;
		grid-template-columns: 5.5em 1fr;
		gap: 10px;
	}

	.modes dt {
		font-weight: 500;
	}

	.modes dd {
		margin: 0;
	}

	.modes .on {
		color: var(--ink);
	}

	.working {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 14px;
		font-size: 13px;
		color: var(--faint);
	}

	.dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}

	.problem {
		margin-top: 10px;
		font-size: 13px;
	}

	/* --- the box to type in --- */
	.dock {
		flex: none;
		padding: 0 0 12px;
	}

	.composer {
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 18px;
		padding: 4px 6px;
		background: var(--bg);
		box-shadow: 0 1px 2px color-mix(in srgb, var(--ink) 6%, transparent), 0 8px 24px -12px color-mix(in srgb, var(--ink) 18%, transparent);
		transition: border-color var(--quick);
	}

	.composer:focus-within {
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

	.chips {
		display: inline-flex;
		gap: 2px;
	}

	.chips button {
		height: 28px;
		padding: 0 11px;
		border-radius: 999px;
		font: 500 13px/1 var(--rb-text);
		color: var(--muted);
	}

	.chips button:hover {
		background: var(--hover);
	}

	.chips button[aria-checked='true'] {
		background: var(--pressed);
		color: var(--ink);
	}

	.switch {
		display: inline-flex;
		align-items: center;
		gap: 7px;
		height: 28px;
		padding: 0 6px;
		font-size: 13px;
		color: var(--muted);
		cursor: pointer;
	}

	.switch input {
		appearance: none;
		position: relative;
		width: 28px;
		height: 16px;
		border-radius: 8px;
		background: var(--line);
		cursor: pointer;
		transition: background-color var(--quick);
	}

	.switch input::after {
		content: '';
		position: absolute;
		top: 2px;
		left: 2px;
		width: 12px;
		height: 12px;
		border-radius: 50%;
		background: var(--bg);
		transition: transform var(--quick);
	}

	.switch input:checked {
		background: var(--accent);
	}

	.switch input:checked::after {
		transform: translateX(12px);
	}

	.go {
		height: 32px;
		padding: 0 14px;
		border-radius: 999px;
		background: var(--ink);
		color: var(--bg);
		font: 600 13px/1 var(--rb-text);
	}

	.go:disabled {
		opacity: 0.3;
	}

	.fine {
		margin-top: 8px;
		padding: 0 10px;
		font-size: 12.5px;
		line-height: 1.45;
		color: var(--faint);
	}

	/* --- what the model wrote --- */
	.prose {
		max-width: 76ch;
		overflow-wrap: anywhere;
		font-size: 15px;
		line-height: 1.6;
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
	.prose :global(.code) {
		margin: 0 0 0.7em;
	}

	.prose :global(h1),
	.prose :global(h2),
	.prose :global(h3),
	.prose :global(h4) {
		margin: 1.2em 0 0.4em;
		font: 600 15px/1.3 var(--rb-text);
	}

	.prose :global(ul),
	.prose :global(ol) {
		padding-left: 1.5em;
	}

	.prose :global(ul) {
		list-style: disc;
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
		background: var(--well);
		font: 0.9em var(--rb-data);
	}

	.prose :global(.code) {
		border-radius: 8px;
		background: var(--well);
		overflow: hidden;
	}

	.prose :global(.code-head) {
		display: flex;
		align-items: center;
		justify-content: space-between;
		height: 26px;
		padding: 0 6px 0 12px;
		font-size: 12px;
		color: var(--faint);
	}

	.prose :global(.code-head button) {
		height: 20px;
		padding: 0 8px;
		border-radius: 999px;
		font: inherit;
		color: var(--muted);
	}

	.prose :global(.code-head button:hover) {
		background: var(--hover);
		color: var(--ink);
	}

	.prose :global(pre) {
		margin: 0;
		padding: 4px 12px 10px;
		overflow-x: auto;
		font: 12.5px/1.55 var(--rb-data);
		tab-size: 2;
	}

	.prose :global(pre code) {
		padding: 0;
		background: none;
		font: inherit;
	}

	@media (max-width: 640px) {
		.turn.you {
			max-width: 92%;
		}
	}
</style>
