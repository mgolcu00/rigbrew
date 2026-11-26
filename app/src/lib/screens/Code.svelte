<script lang="ts">
	import { onMount, tick, untrack } from 'svelte';
	import { copyText, isTauri, openExternal, type AgentToolStep } from '#lib/api.ts';
	import { shownArguments } from '#lib/chattools.ts';
	import { code, isActive, modes, purposeOf } from '#lib/code.svelte.ts';
	import { activityOf, diffCount, fold, lineOf, summary, tails } from '#lib/code-view.ts';
	import { appCommand, contextShown, thoughtFor } from '#lib/code-compose.ts';
	import { WORD_MIN_MS, WORD_MS, elapsed, nextWord, type Activity } from '#lib/code-words.ts';
	import { connectors } from '#lib/connectors.svelte.ts';
	import { ago } from '#lib/format.ts';
	import { renderMarkdown } from '#lib/markdown.ts';
	import { remembered } from '#lib/remember.ts';
	import { skills } from '#lib/skills.svelte.ts';
	import { app } from '#lib/state.svelte.ts';
	import { isMac, machine, mod } from '#lib/words.ts';
	import CodeComposer from '#lib/components/CodeComposer.svelte';
	import CodePreview from '#lib/components/CodePreview.svelte';
	import ErrorLine from '#lib/components/ErrorLine.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import ModeMenu from '#lib/components/ModeMenu.svelte';
	import Shot from '#lib/components/Shot.svelte';

	// Code: pick a folder, say what should be done, and watch a model on this computer do it one
	// step at a time. It reads like a good terminal agent: what you said, then one compact line
	// per thing done (a run of look-ups is a single line that opens), the plan as a checklist,
	// the answer as prose, and while it works one line above the box with a word, the time so
	// far and how to stop. A change to a file or a command stops at a card and waits for a yes.

	let draft = $state('');
	/** Pictures pasted or dropped into the box: PNG data addresses. */
	let pictures = $state<string[]>([]);
	let typed = $state(code.folder);
	let composer = $state<CodeComposer>();
	let thread = $state<HTMLElement>();

	const task = $derived(code.task);
	const working = $derived(isActive(task));
	// The model that does the work: the one chosen here, else the heaviest chat model installed,
	// since the bigger one writes better code. Small things (titles) stay with whatever is loaded.
	const weight = (p: (typeof app.chatPackages)[number]) => Math.max(0, ...p.variants.filter((v) => p.installed.includes(v.id)).map((v) => v.disk_mb));
	let chosenModel = $state(remembered.get('code.model') ?? '');
	const model = $derived(app.chatPackages.find((p) => p.id === chosenModel) ?? [...app.chatPackages].sort((a, b) => weight(b) - weight(a))[0] ?? null);
	function chooseModel(id: string) {
		chosenModel = id;
		remembered.set('code.model', id);
	}
	/** The model takes pictures: its installed build has a vision part. */
	const sees = $derived(Boolean(model?.variants.some((v) => model.installed.includes(v.id) && v.vision)));
	const choices = $derived({ model: app.chatPackages.map((p) => ({ id: p.id, name: p.name })), mode: modes.map((m) => ({ id: m.id, name: m.name })) });
	/** "34% of context", from the engine's count at the last turn. */
	const fullness = $derived(contextShown(task?.context));
	// The name of the open task: click it to change it. A name typed here is never written over.
	let renaming = $state(false);
	let newTitle = $state('');
	let titleField = $state<HTMLInputElement>();
	async function startRename() {
		if (!task) return;
		newTitle = task.title;
		renaming = true;
		await tick();
		titleField?.select();
	}
	function finishRename(keep: boolean) {
		if (!renaming) return;
		renaming = false;
		if (keep && task) void code.rename(task.id, newTitle);
	}

	const folderName = $derived(code.folder.split(/[/\\]/).filter(Boolean).pop() ?? code.folder);
	const nameOf = (folder: string) => folder.split(/[/\\]/).filter(Boolean).pop() ?? folder;
	const others = $derived(code.recent.filter((f) => f !== code.folder));
	const plan = $derived(task?.plan ?? []);
	const planDone = $derived(plan.filter((s) => s.status === 'done').length);
	const rows = $derived(task ? fold(task.steps) : []);
	/** The model is at work right now: not waiting for a yes, not finished. */
	const busy = $derived(task?.state === 'running');
	/** The task has something to show next to it: an app it started, or a phone's screen. */
	const previewable = $derived(Boolean(task && (task.app || task.device)));

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

	/** A command the app does itself: a new task in this folder, another model, other permissions. */
	function onapp(name: 'clear' | 'model' | 'mode', value?: string) {
		const named = (list: { id: string; name: string }[]) => list.find((c) => c.id === value || c.name.toLowerCase().startsWith((value ?? '').toLowerCase()))?.id;
		if (name === 'clear') {
			code.close();
			pictures = [];
		} else if (name === 'model' && value && named(choices.model)) chooseModel(named(choices.model) ?? '');
		else if (name === 'mode' && value && named(choices.mode)) code.setMode(named(choices.mode) as (typeof modes)[number]['id']);
		draft = '';
		composer?.focus();
	}

	// While it works a message waits for the next turn; the service puts it in then.
	async function send() {
		const own = appCommand(draft);
		if (own) return onapp(own.name as 'clear' | 'model' | 'mode', own.rest);
		if (!draft.trim() && !(task && pictures.length)) return;
		if (await code.send(draft, model?.id, pictures)) {
			draft = '';
			pictures = [];
		}
	}

	// --- the line above the box while it works: a word, the time so far, how to stop -----------

	let word = $state('');
	let now = $state(Date.now());
	/** When the request being worked on was sent. */
	let started = $state(0);
	$effect(() => {
		if (!busy) {
			word = '';
			return;
		}
		const since = Date.now();
		const recent: string[] = [];
		let shownFor: Activity = 'thinking';
		let wordAt = 0;
		// The word changes every few seconds, and a little sooner when something else starts
		// happening. The steps themselves come and go much faster than anyone could read.
		const turn = () => {
			// Written, never read here: reading it would make this effect start over on every tick.
			const at = Date.now();
			now = at;
			const doing = untrack(() => (task ? activityOf(task.steps) : 'thinking'));
			if (wordAt && at - wordAt < (doing === shownFor ? WORD_MS : WORD_MIN_MS)) return;
			word = nextWord(doing, recent);
			recent.push(word);
			shownFor = doing;
			wordAt = at;
		};
		started = untrack(() => task?.request_ms) || since;
		turn();
		const timer = setInterval(turn, 500);
		return () => clearInterval(timer);
	});

	// Keys that work anywhere on the screen: Cmd or Ctrl with Enter allows the step that waits,
	// Esc stops the work where it is (Continue takes it up again).
	function onkey(event: KeyboardEvent) {
		if (event.isComposing || !task) return;
		if (event.key === 'Enter' && (event.metaKey || event.ctrlKey)) {
			if (task.pending && !code.busy) {
				event.preventDefault();
				code.answer(true);
			}
		} else if (event.key === 'Escape' && working) {
			event.preventDefault();
			stop();
		}
	}

	async function stop() {
		await code.stop();
	}

	/** The work ended before it was done: it can go on from where it was. */
	const resumable = $derived(Boolean(task && !working && (task.state === 'stopped' || task.state === 'interrupted' || task.state === 'failed')));

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
			const lines = diffCount(step.diff);
			file.plus += lines.plus;
			file.minus += lines.minus;
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

	/** The tools whose line does not say everything that was asked for. */
	const showsArguments = (step: AgentToolStep) =>
		step.name === 'look_at_page' ? Array.isArray(step.arguments.do) : !['start_app', 'check_project', 'simulator', 'read_file', 'list_files', 'search', 'outline', 'web_search', 'fetch_url', 'run_command', 'edit_file', 'write_file', 'append_file', 'use_skill', 'read_skill_file'].includes(step.name);

	const states: Record<string, string> = {
		running: 'Working',
		waiting: 'Waiting for you',
		asking: 'Waiting for you',
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

{#snippet stepRow(step: AgentToolStep)}
	{@const l = lineOf(step)}
	<li class="step {step.status}">
		<details>
			<summary>
				<span class="mark" aria-hidden="true"></span>
				<span class="verb">{l.verb}</span>
				{#if l.code}<code class="what">{l.code}</code>{/if}
				{#if l.plus !== undefined}
					<span class="plus" aria-label="{l.plus} lines added">+{l.plus}</span>
					<span class="minus" aria-label="{l.minus} lines removed">−{l.minus}</span>
				{/if}
				{#if tails[step.status] && !l.quiet}<span class="tail">{tails[step.status]}</span>{/if}
			</summary>
			<div class="inside">
				{#if step.image && task}<Shot task={task.id} name={step.image} alt="What it saw" small />{/if}
				{#if step.diff}{@render diffView(step.diff)}{/if}
				{#if showsArguments(step)}<pre class="raw selectable">{step.name} {shown(step)}</pre>{/if}
				{#if step.result}<pre class="raw selectable">{step.result}</pre>{/if}
			</div>
		</details>
	</li>
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

{#snippet composerBox(placeholder: string, below = false)}
	<CodeComposer bind:this={composer} bind:draft bind:pictures {placeholder} {below} disabled={!model} folder={code.folder} skills={code.skills} {sees} {choices} onsend={send} {onapp}>
		{#snippet tools()}
			<ModeMenu {modes} value={code.mode} onchange={(m) => code.setMode(m)} danger="bypass" />
			{#if app.chatPackages.length > 1 && model}
				<label class="perm" title="The model that does the work">
					<span class="sr">Model</span>
					<select value={model.id} onchange={(e) => chooseModel(e.currentTarget.value)} aria-label="Model">
						{#each app.chatPackages as p (p.id)}
							<option value={p.id}>{p.name}</option>
						{/each}
					</select>
				</label>
			{/if}
			<label class="switch" title="Lets the model search the web and open pages. Search words leave this computer; your files do not.">
				<input type="checkbox" checked={code.web} onchange={(e) => code.setWeb(e.currentTarget.checked)} />
				<span>Web</span>
			</label>
			{#if connectors.list.length > 0}
				<label class="switch" title="Lets the model use the tools of your running connectors. It asks you each time.">
					<input type="checkbox" checked={code.connectors} onchange={(e) => code.setConnectors(e.currentTarget.checked)} />
					<span>Connectors</span>
				</label>
			{/if}
			<span class="grow"></span>
			{#if fullness && !busy}<span class="meter" title="How much of what the model can hold in mind the task used at its last turn">{fullness}</span>{/if}
			{#if working}
				{#if draft.trim() || pictures.length}<button class="go" onclick={send} disabled={code.busy} title="Goes in at the next turn">Queue</button>{/if}
				<button class="go stop" onclick={stop} title="Stops the work where it is (Esc)">Stop</button>
			{:else}
				<button class="go" onclick={send} disabled={(!draft.trim() && !(task && pictures.length)) || !model || code.busy}>Send</button>
			{/if}
		{/snippet}
	</CodeComposer>
{/snippet}

<section class="screen code" class:split={previewable && code.preview} aria-label="Code">
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
					{#if code.problem}<ErrorLine error={code.problem} where="code" />{/if}
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
					{#if renaming}
						<input
							class="title rename"
							bind:this={titleField}
							bind:value={newTitle}
							aria-label="Name of the task"
							maxlength="80"
							onkeydown={(e) => {
								e.stopPropagation();
								if (e.key === 'Enter') finishRename(true);
								else if (e.key === 'Escape') finishRename(false);
							}}
							onblur={() => finishRename(true)}
						/>
					{:else}
						<button class="title" onclick={startRename} title="Rename this task" aria-label="Task name: {task.title}. Rename">{task.title}</button>
					{/if}
					<span class="state" class:live={working}>{states[task.state] ?? task.state}</span>
					{#if previewable && !code.preview}
						<button class="back" onclick={() => code.setPreview(true)} title="Show what the task made"><Icon name="pane" size={15} />Preview</button>
					{/if}
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
					<summary>Plan <span class="count">{planDone}/{plan.length}</span></summary>
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
						{#each rows as row (row.at)}
							{#if row.kind === 'user'}
								<li class="turn you">
									{#if row.text}<p class="said selectable">{row.text}</p>{/if}
									{#if row.images?.length}<div class="shots">{#each row.images as image (image)}<Shot task={task.id} name={image} alt="A picture you sent" small />{/each}</div>{/if}
								</li>
							{:else if row.kind === 'thought'}
								<li class="step thought">
									<details>
										<summary><span class="mark" aria-hidden="true"></span><span class="verb">{thoughtFor(row.ms)}</span></summary>
										<div class="inside"><p class="thinking selectable">{row.text}</p></div>
									</details>
								</li>
							{:else if row.kind === 'model' || row.kind === 'question'}
								<li class="turn model" class:question={row.kind === 'question'}>
									<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
									<div class="prose selectable" onclick={onanswerclick}>{@html renderMarkdown(row.text)}</div>
								</li>
							{:else if row.kind === 'note'}
								<li class="note">{row.text}</li>
							{:else if row.kind === 'lookups'}
								{@const sum = summary(row.steps)}
								<li class="step" class:running={sum.running}>
									<details>
										<summary>
											<span class="mark" aria-hidden="true"></span>
											{#if sum.running}
												<span class="verb">{sum.label}</span>
												{#if row.steps.length > 1}<span class="n">{row.steps.length}</span>{/if}
											{:else}
												{#each sum.parts as part (part.label)}
													<span class="part"><span class="verb">{part.label}</span>{#if part.count}<span class="n">{part.count}</span>{/if}</span>
												{/each}
												{#if sum.failed}<span class="tail">{sum.failed} failed</span>{/if}
											{/if}
										</summary>
										<ol class="sub">
											{#each row.steps as step (step.call)}{@render stepRow(step)}{/each}
										</ol>
									</details>
								</li>
							{:else if row.kind === 'step'}
								{@render stepRow(row.step)}
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
								<h2>{p.tool === 'start_app' ? 'Start the app with this command?' : p.tool === 'check_project' ? 'Run the checks of this project?' : 'Run this command?'}</h2>
								<pre class="raw selectable">{p.command}</pre>
								{#if purposeOf(p.command)}<p class="purpose">{purposeOf(p.command)}</p>{/if}
								{#if !isMac}<p class="why">It runs with your own rights.</p>{/if}
							{:else if p.tool === 'call_tool'}
								<h2>Use {asked?.kind === 'tool' ? String(asked.arguments.tool ?? 'a tool') : 'a tool'} from the connector {p.path}?</h2>
								<pre class="raw selectable">{shownArguments(asked?.kind === 'tool' ? asked.arguments.arguments : undefined)}</pre>
								<p class="why">This tool comes from a connector and works outside this folder.</p>
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
						</div>
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
								<p class="fine">A command ran. It may have changed files.</p>
							{/if}
							{#if undoable && undoable.request === requests}
								<div class="answers">
									<button
										class="plainbtn"
										onclick={() => code.undo()}
										disabled={code.busy}
										title={undoable.kind === 'git'
											? 'Every file goes back to how it was before this request. Files that git ignores stay as they are.'
											: undoable.commands
												? 'The files above go back to how they were. What commands changed cannot be undone, because this folder is not a git repository.'
												: 'The files above go back to how they were before this request.'}>Undo</button
									>
									{#if undoable.kind !== 'git' && undoable.commands}<span class="fine">Not what commands changed</span>{/if}
								</div>
							{/if}
						</div>
					{:else if !working && undoable && undoable.request !== requests}
						<div class="answers earlier">
							<button class="plainbtn" onclick={() => code.undo()} disabled={code.busy} title="Takes back what request {undoable.request} of this task changed">Undo the earlier changes</button>
						</div>
					{/if}

					{#if task.error}{#if task.state === 'failed'}<ErrorLine error={task.error} where="code" />{:else}<p class="ended selectable">{task.error}</p>{/if}
					{:else if task.state === 'stopped'}<p class="ended">Stopped</p>{/if}
					{#if resumable}
						<div class="answers earlier"><button class="plainbtn" onclick={() => code.goOn()} disabled={code.busy} title="Goes on from where it was">Continue</button></div>
					{/if}
					{#if code.problem}<ErrorLine error={code.problem} where="code" />{/if}
				</div>
			</div>
			<div class="dock">
				<div class="column">
					<!-- Said once to a screen reader; the words that change are for the eye only. -->
					<span class="sr" role="status">{busy ? 'Working' : ''}</span>
					{#if busy}
						<div class="status">
							<span class="brew" aria-hidden="true"><i></i><i></i><i></i></span>
							<span class="word" aria-hidden="true">{word}</span>
							<span class="time" aria-hidden="true">{elapsed(now - started)}</span>
							<span class="grow"></span>
							{#if fullness}<span class="time">{fullness}</span>{/if}
							<span class="esc" aria-hidden="true">esc to interrupt</span>
						</div>
					{/if}
					{#if task.queued?.length}
						<ul class="queued" aria-label="Waiting for the next turn">
							{#each task.queued as q (q.id)}
								<li>
									<span class="tag">Queued</span>
									<span class="said">{q.text || 'A picture'}</span>
									<button class="drop" onclick={() => code.unqueue(q.id)} title="Take it back" aria-label="Take back the queued message">×</button>
								</li>
							{/each}
						</ul>
					{/if}
					{@render composerBox(task.pending ? 'Answer above first' : working ? 'Queue a message' : task.state === 'asking' ? 'Your answer' : 'Ask for the next thing')}
				</div>
			</div>
		{:else}
			<div class="thread scroll">
				<div class="column start">
					<h1>What should be done in {folderName}?</h1>
					{#if !model}
						<p class="lede">Code needs a chat model. <button class="link" onclick={() => app.go('models')}>Install one under Models</button>, then come back.</p>
					{/if}
					{@render composerBox('Say it in plain words', true)}
					{#if code.instructions.length}<p class="reads" title="The model follows the instructions in these files">{code.instructions.join(' · ')}</p>{/if}
					{#if code.problem}<ErrorLine error={code.problem} where="code" />{/if}

					{#if code.tasks.length === 0}
						<ul class="ideas">
							{#each ideas as idea (idea)}
								<li><button onclick={() => ((draft = idea), composer?.focus())} disabled={!model}>{idea}</button></li>
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
	{#if task && previewable && code.preview}
		<div class="side"><CodePreview {task} /></div>
	{/if}
</section>

<style>
	.code {
		position: relative;
		font-family: var(--rb-text);
	}

	/* The preview stands to the right of the work; the work keeps the rest. */
	.code.split {
		padding-right: min(46%, 640px);
	}

	.side {
		position: absolute;
		inset: 0 0 0 auto;
		width: min(46%, 640px);
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

	button.title {
		height: 28px;
		margin: 0 -6px;
		padding: 0 6px;
		border-radius: 8px;
		text-align: left;
	}

	button.title:hover {
		background: var(--hover);
		color: var(--ink);
	}

	.title.rename {
		height: 28px;
		margin: 0 -6px;
		padding: 0 6px;
		border: 0;
		border-radius: 8px;
		box-shadow: inset 0 0 0 1px var(--accent);
		background: var(--bg);
		color: var(--ink);
	}

	.title.rename:focus-visible {
		outline: none;
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

	.verb {
		flex: none;
	}

	/* What the step was about: a path, a command, a query. Cut with an ellipsis, never wrapped. */
	.what {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 12.5px var(--rb-data);
		color: var(--ink);
	}

	.n,
	.tail,
	.step .plus,
	.step .minus {
		flex: none;
		font-size: 12px;
		color: var(--faint);
		font-variant-numeric: tabular-nums;
	}

	.step .plus,
	.step .minus {
		font-family: var(--rb-data);
	}

	.step .plus {
		color: var(--fit);
	}

	.step .minus {
		color: var(--fail);
	}

	.part {
		display: inline-flex;
		align-items: baseline;
		gap: 6px;
		min-width: 0;
		white-space: nowrap;
	}

	.part + .part::before {
		content: '·';
		color: var(--faint);
	}

	.inside {
		display: grid;
		gap: 6px;
		margin: 6px 0 10px 15px;
		padding-left: 12px;
		border-left: 1px solid var(--line-soft);
	}

	/* The look-ups of one run, each its own line that opens. */
	.sub {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		margin: 2px 0 6px 2px;
		padding: 0 0 0 13px;
		border-left: 1px solid var(--line-soft);
		list-style: none;
	}

	.sub .step {
		margin: 0;
	}

	.turn.you .shots {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin-top: 6px;
	}

	.turn.you .said + .shots {
		margin-top: 8px;
	}

	.thinking {
		max-height: 320px;
		overflow: auto;
		font-size: 13px;
		line-height: 1.55;
		color: var(--muted);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.question .prose {
		padding-left: 12px;
		border-left: 2px solid var(--accent);
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

	.purpose {
		font-size: 13px;
		line-height: 1.45;
		color: var(--muted);
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

	/* --- while it works: a small mark, a word that changes, the time so far, how to stop --- */
	.status {
		display: flex;
		align-items: center;
		gap: 9px;
		min-height: 24px;
		margin-bottom: 6px;
		padding: 0 10px;
		font-size: 13px;
		color: var(--muted);
	}

	/* Three bubbles rising, as in a pot that is nearly there. */
	.brew {
		flex: none;
		display: inline-flex;
		align-items: flex-end;
		gap: 2px;
		width: 13px;
		height: 12px;
	}

	.brew i {
		width: 3px;
		height: 3px;
		border-radius: 50%;
		background: var(--accent);
		animation: bubble 1.3s ease-in-out infinite;
	}

	.brew i:nth-child(2) {
		animation-delay: 0.22s;
	}

	.brew i:nth-child(3) {
		animation-delay: 0.44s;
	}

	@keyframes bubble {
		0%,
		100% {
			transform: translateY(0);
			opacity: 0.35;
		}
		40% {
			transform: translateY(-7px);
			opacity: 1;
		}
	}

	.word {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--accent);
	}

	.time,
	.esc {
		flex: none;
		font-size: 12px;
		color: var(--faint);
		font-variant-numeric: tabular-nums;
	}

	/* The word still changes; the mark stands still. */
	@media (prefers-reduced-motion: reduce) {
		.brew i {
			animation: none;
			opacity: 0.8;
		}
	}

	/* --- the box to type in (see CodeComposer) --- */
	.dock {
		flex: none;
		padding: 0 0 12px;
	}

	.meter {
		flex: none;
		font-size: 12px;
		color: var(--faint);
		font-variant-numeric: tabular-nums;
	}

	.reads {
		justify-self: center;
		font: 12px var(--rb-data);
		color: var(--faint);
	}

	/* Messages that go in at the next turn. */
	.queued {
		display: grid;
		gap: 4px;
		margin: 0 0 6px;
		padding: 0 10px;
		list-style: none;
	}

	.queued li {
		display: flex;
		align-items: baseline;
		gap: 8px;
		min-width: 0;
		font-size: 13px;
		color: var(--muted);
	}

	.queued .tag {
		flex: none;
		font-size: 12px;
		color: var(--faint);
	}

	.queued .said {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 13px;
	}

	.queued .drop {
		flex: none;
		width: 20px;
		height: 20px;
		border-radius: 6px;
		color: var(--faint);
	}

	.queued .drop:hover {
		background: var(--hover);
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

	/* The model picker: one quiet menu in the box where you type, like the permission picker (ModeMenu). */
	.perm {
		display: inline-flex;
		align-items: center;
	}

	.perm select {
		height: 28px;
		padding: 0 8px;
		border: 0;
		border-radius: 999px;
		background: var(--pressed);
		color: var(--ink);
		font: 500 13px/1 var(--rb-text);
		cursor: pointer;
	}

</style>
