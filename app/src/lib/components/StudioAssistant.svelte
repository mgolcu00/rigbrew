<script lang="ts">
	import { tick, untrack } from 'svelte';
	import { agentShot, type AgentToolStep } from '#lib/api.ts';
	import { assistant } from '#lib/assistant.svelte.ts';
	import { diffRows, filmActivity, filmLine, filmModes, firstPrompts, foldFilm, readsLine } from '#lib/assistant-view.ts';
	import { WORD_MIN_MS, WORD_MS, elapsed, nextWord, type Activity } from '#lib/code-words.ts';
	import { renderMarkdown } from '#lib/markdown.ts';
	import { app } from '#lib/state.svelte.ts';
	import { ASSISTANT_STEP } from '#lib/studio-edit.ts';
	import { tag } from '#lib/studio-tags.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { mod } from '#lib/words.ts';
	import ErrorLine from './ErrorLine.svelte';
	import ModeMenu from './ModeMenu.svelte';
	import StudioIcon from './StudioIcon.svelte';

	// The assistant next to the tracks: say what you want and the model does it on the film, one
	// quiet line for each thing it did (open one to see before and after), the plan as a
	// checklist, a line with a word and the time while it works, and a card when it wants to
	// make shots, dub or export.

	let draft = $state('');
	let input = $state<HTMLTextAreaElement>();
	let thread = $state<HTMLElement>();

	const task = $derived(assistant.task);
	const rows = $derived(task ? foldFilm(task.steps) : []);
	const plan = $derived(task?.plan ?? []);
	const planDone = $derived(plan.filter((s) => s.status === 'done').length);
	/** The model is at work right now: not waiting for a yes. */
	const busy = $derived(task?.state === 'running');
	const model = $derived(assistant.model);
	const models = $derived(app.chatPackages.map((p) => ({ id: p.id, name: p.name, does: 'The model that does the work' })));
	/** The shots selected in the tracks, by number: a request is about them. */
	const selected = $derived(
		studio.shotsSel
			.map((id) => (studio.doc?.shots.findIndex((s) => s.id === id) ?? -1) + 1)
			.filter((n) => n > 0)
			.sort((a, b) => a - b)
	);
	/** The newest request changed the film and is the step an undo takes back now. */
	const undoable = $derived.by(() => {
		void studio.edits;
		if (!task || assistant.working || studio.history.undoLabel !== ASSISTANT_STEP) return false;
		const from = task.steps.findLastIndex((s) => s.kind === 'user');
		return task.steps.slice(from).some((s) => s.kind === 'tool' && s.status === 'done' && s.name !== 'update_plan' && !['read_film', 'read_shot', 'look_at_shot'].includes(s.name));
	});

	// Follow the work: the newest step stays in view.
	$effect(() => {
		void task?.steps.length;
		void task?.pending;
		void task?.state;
		void assistant.writing;
		tick().then(() => thread?.scrollTo({ top: thread.scrollHeight }));
	});

	function grow() {
		if (!input) return;
		input.style.height = 'auto';
		input.style.height = `${Math.min(input.scrollHeight, 140)}px`;
	}

	async function send() {
		if (assistant.working || !draft.trim()) return;
		if (await assistant.send(draft, [...studio.shotsSel])) {
			draft = '';
			await tick();
			grow();
		}
	}

	function onkeydown(e: KeyboardEvent) {
		if (e.isComposing) return;
		if (e.key === 'Enter' && !e.shiftKey) {
			e.preventDefault();
			if (task?.pending && (e.metaKey || e.ctrlKey)) assistant.answer(true);
			else send();
		} else if (e.key === 'Escape') {
			e.preventDefault();
			if (!assistant.escape()) input?.blur();
		}
	}

	function suggest(text: string) {
		draft = text;
		tick().then(() => {
			input?.focus();
			grow();
		});
	}

	// --- the line above the box while it works: a word, the time so far, how to stop -----------

	let word = $state('');
	let now = $state(Date.now());
	let started = $state(0);
	$effect(() => {
		if (!busy) {
			word = '';
			return;
		}
		const recent: string[] = [];
		let shownFor: Activity = 'thinking';
		let wordAt = 0;
		const turn = () => {
			const at = Date.now();
			now = at;
			const doing = untrack(() => (task ? filmActivity(task.steps) : 'thinking'));
			if (wordAt && at - wordAt < (doing === shownFor ? WORD_MS : WORD_MIN_MS)) return;
			word = nextWord(doing, recent);
			recent.push(word);
			shownFor = doing;
			wordAt = at;
		};
		started = untrack(() => task?.request_ms) || Date.now();
		turn();
		const timer = setInterval(turn, 500);
		return () => clearInterval(timer);
	});

	// --- pictures the assistant looked at ---------------------------------------------------------

	let pictures = $state<Record<string, string>>({});
	async function picture(step: AgentToolStep) {
		const name = step.image;
		if (!task || !name || pictures[name]) return;
		try {
			pictures = { ...pictures, [name]: URL.createObjectURL(await agentShot(task.id, name)) };
		} catch {
			// The picture is gone (an old task); the line says what was looked at.
		}
	}

	/** What opening a step shows besides before and after: the rest of what it answered. */
	const rest = (step: AgentToolStep) => (step.status === 'done' ? (step.result ?? '').split('\n').slice(1).join('\n').trim() : (step.result ?? '').trim());

	const states: Record<string, string> = { failed: 'Did not finish', stopped: 'Stopped', interrupted: 'Cut off' };
</script>

{#snippet stepRow(step: AgentToolStep)}
	{@const line = filmLine(step)}
	{@const diff = diffRows(step.diff)}
	{@const more = rest(step)}
	<li class="step" class:running={step.status === 'running'} class:off={line.tail !== ''}>
		<details ontoggle={(e) => e.currentTarget.open && picture(step)}>
			<summary>
				<span class="mark" aria-hidden="true"></span>
				<span class="txt">{line.text}</span>
				{#if line.tail}<span class="tail">{line.tail}</span>{/if}
			</summary>
			{#if diff.length || more || step.image}
				<div class="open">
					{#each diff as d, i (i)}
						<p class="d {d.sign === '-' ? 'was' : 'is'} selectable"><span class="sign" aria-hidden="true">{d.sign === '-' ? '−' : '+'}</span>{d.text || ' '}</p>
					{/each}
					{#if more}<p class="more selectable">{more}</p>{/if}
					{#if step.image && pictures[step.image]}<img src={pictures[step.image]} alt="What the assistant looked at" />{/if}
				</div>
			{/if}
		</details>
	</li>
{/snippet}

<div class="assistant" aria-label="Assistant">
	<div class="head">
		<span class="title" title={task?.title ?? ''}>{task?.title ?? 'Assistant'}</span>
		{#if task && !assistant.working}
			<button class="st-btn quiet icon" onclick={() => assistant.fresh()} use:tag={{ name: 'New conversation', what: 'Starts over about this film; the film stays as it is.' }} aria-label="New conversation"><StudioIcon name="plus" size={12} /></button>
		{/if}
	</div>

	{#if plan.length}
		<details class="plan" open>
			<summary>Plan <span class="count">{planDone}/{plan.length}</span></summary>
			<ol>
				{#each plan as s, i (i)}
					<li class={s.status} aria-current={s.status === 'doing' ? 'step' : undefined}>
						<span class="box" aria-hidden="true"></span>
						<span class="what">{s.text}</span>
					</li>
				{/each}
			</ol>
		</details>
	{/if}

	<div class="thread st-scroll" bind:this={thread}>
		{#if task}
			<ol class="rows">
				{#each rows as row (row.at)}
					{#if row.kind === 'user'}
						<li class="you selectable">{row.text}</li>
					{:else if row.kind === 'model' || row.kind === 'question'}
						<li class="said selectable" class:question={row.kind === 'question'}>{@html renderMarkdown(row.text)}</li>
					{:else if row.kind === 'note'}
						<li class="note">{row.text}</li>
					{:else if row.kind === 'reads'}
						<li class="step quiet" class:running={row.steps.some((s) => s.status === 'running')}>
							<details ontoggle={(e) => e.currentTarget.open && row.steps.forEach(picture)}>
								<summary>
									<span class="mark" aria-hidden="true"></span>
									<span class="txt">{readsLine(row.steps)}</span>
								</summary>
								<div class="open">
									{#each row.steps as s (s.call)}
										{#if s.image && pictures[s.image]}<img src={pictures[s.image]} alt="Shot {String(s.arguments.shot ?? '')}" />{:else}<p class="more selectable">{(s.result ?? '').trim()}</p>{/if}
									{/each}
								</div>
							</details>
						</li>
					{:else if row.kind === 'step'}
						{@render stepRow(row.step)}
					{/if}
				{/each}
				{#if busy && assistant.writing}
					<li class="said writing selectable">{@html renderMarkdown(assistant.writing)}</li>
				{/if}
			</ol>

			{#if task.pending}
				<div class="ask" role="group" aria-label="Waiting for your answer">
					<p class="q selectable">{task.pending.command ?? task.pending.tool}?</p>
					<div class="answers">
						<button class="st-btn primary" onclick={() => assistant.answer(true)} disabled={assistant.busy} use:tag={{ name: 'Go ahead', what: 'Lets the assistant do this.', key: `${mod}Return` }}>Go ahead</button>
						<button class="st-btn" onclick={() => assistant.answer(false)} disabled={assistant.busy} use:tag={{ name: 'No', what: 'The assistant does not do this and goes on without it.' }}>No</button>
						<span class="grow"></span>
						<button class="st-btn quiet" onclick={() => assistant.answer(true, true)} disabled={assistant.busy} use:tag={{ name: 'Always', what: 'Does this without asking for the rest of this conversation.' }}>Always</button>
					</div>
				</div>
			{/if}

			{#if undoable}
				<div class="undo"><button class="st-btn quiet" onclick={() => studio.undo()} use:tag={{ id: 'undo-request', key: `${mod}Z` }}><StudioIcon name="undo" size={12} />Undo what the assistant did</button></div>
			{/if}

			{#if task.state === 'failed' && task.error}<ErrorLine error={task.error} compact where="studio" />
			{:else if states[task.state]}<p class="ended">{task.error ?? states[task.state]}</p>{/if}
		{:else}
			<div class="start">
				{#if !model}
					<div class="empty">
						<StudioIcon name="ask" size={18} />
						<p>The assistant needs a chat model to understand what you ask.</p>
						<button class="st-btn primary" onclick={() => app.go('models')}>Add one under Models</button>
					</div>
				{:else}
					<div class="empty">
						<StudioIcon name="ask" size={18} />
						<p>Say what to change. The assistant edits the film while you watch.</p>
						<button class="st-btn primary" onclick={() => input?.focus()}>Ask the assistant</button>
					</div>
					<p class="or">Or start from one of these:</p>
					<ul class="firsts">
						{#each firstPrompts as idea (idea)}
							<li><button onclick={() => suggest(idea)}>{idea}</button></li>
						{/each}
					</ul>
				{/if}
			</div>
		{/if}
		{#if assistant.problem}<ErrorLine error={assistant.problem} compact where="studio" />{/if}
	</div>

	<div class="dock">
		<span class="sr" role="status">{busy ? 'Working' : ''}</span>
		{#if busy}
			<div class="status" aria-hidden="true">
				<span class="dot"></span>
				<span class="word">{word}</span>
				<span class="time">{elapsed(now - started)}</span>
				<span class="grow"></span>
				<span class="esc" class:armed={assistant.stopping}>{assistant.stopping ? 'esc again to stop' : 'esc to stop'}</span>
			</div>
		{/if}
		<div class="composer" class:off={!model}>
			{#if selected.length}
				<span class="sel">{selected.length === 1 ? `Shot ${selected[0]}` : `Shots ${selected.join(', ')}`} selected</span>
			{/if}
			<textarea
				bind:this={input}
				bind:value={draft}
				oninput={grow}
				{onkeydown}
				rows="1"
				spellcheck="false"
				disabled={!model}
				placeholder={task?.pending ? 'Answer above first' : task?.state === 'asking' ? 'Your answer' : task ? 'Ask for the next thing' : 'Say what to do with the film'}
				aria-label="What should be done with the film"
			></textarea>
			<div class="tools">
				<ModeMenu modes={filmModes} value={assistant.mode} onchange={(m) => assistant.setMode(m)} danger="bypass" />
				{#if models.length > 1 && model}
					<ModeMenu modes={models} value={model.id} onchange={(id) => assistant.chooseModel(id)} label="Model" />
				{/if}
				<span class="grow"></span>
				{#if assistant.working}
					<button class="st-btn" onclick={() => assistant.stop()} use:tag={'stop-assistant'}>Stop</button>
				{:else}
					<button class="st-btn primary" onclick={send} disabled={!draft.trim() || !model || assistant.busy} use:tag={{ id: 'ask', name: 'Send', what: draft.trim() ? 'Sends your request; the assistant edits the film step by step.' : 'Type what to change first.' }}>Send</button>
				{/if}
			</div>
		</div>
	</div>
</div>

<style>
	.assistant {
		--pressed: rgba(255, 255, 255, 0.08);
		--warn: var(--st-warn);
		--warn-soft: rgba(224, 169, 63, 0.16);
		flex: 1 1 0;
		min-height: 0;
		display: flex;
		flex-direction: column;
		background: var(--st-panel);
		font-size: 12px;
		line-height: 1.45;
	}

	.head {
		flex: none;
		display: flex;
		align-items: center;
		gap: 6px;
		height: 28px;
		padding: 0 6px 0 10px;
		border-bottom: 1px solid var(--st-edge);
	}

	.title {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
		font-weight: 600;
		color: var(--st-muted);
	}

	/* --- the plan --- */
	.plan {
		flex: none;
		padding: 6px 10px;
		border-bottom: 1px solid var(--st-edge);
	}

	.plan summary {
		cursor: pointer;
		font: 600 10px/1.6 var(--rb-text);
		letter-spacing: 0.07em;
		text-transform: uppercase;
		color: var(--st-faint);
	}

	.plan .count {
		margin-left: 4px;
		font-family: var(--rb-data);
	}

	.plan ol {
		margin: 4px 0 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 2px;
	}

	.plan li {
		display: flex;
		gap: 7px;
		align-items: baseline;
		color: var(--st-muted);
	}

	.plan .box {
		flex: none;
		width: 8px;
		height: 8px;
		border-radius: 2px;
		box-shadow: inset 0 0 0 1px var(--st-faint);
		transform: translateY(1px);
	}

	.plan li.done {
		color: var(--st-faint);
		text-decoration: line-through;
	}

	.plan li.done .box {
		background: var(--st-ok);
		box-shadow: none;
	}

	.plan li.doing {
		color: var(--st-ink);
	}

	.plan li.doing .box {
		box-shadow: inset 0 0 0 1.5px var(--st-accent);
	}

	/* --- the conversation --- */
	.thread {
		flex: 1 1 0;
		min-height: 0;
		padding: 8px 10px;
	}

	.rows {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 5px;
	}

	.you {
		justify-self: end;
		max-width: 88%;
		margin-top: 6px;
		padding: 5px 9px;
		border-radius: 8px;
		background: var(--st-raised);
		color: var(--st-ink);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.said {
		color: var(--st-ink);
		overflow-wrap: anywhere;
	}

	.said :global(p) {
		margin: 0 0 4px;
	}

	.said :global(ul),
	.said :global(ol) {
		margin: 0 0 4px;
		padding-left: 18px;
	}

	.said.question {
		padding-left: 8px;
		box-shadow: inset 2px 0 0 var(--st-accent);
	}

	.said.writing {
		color: var(--st-muted);
	}

	.note {
		color: var(--st-faint);
		font-size: 11px;
	}

	.step summary {
		display: flex;
		align-items: baseline;
		gap: 7px;
		min-width: 0;
		cursor: pointer;
		list-style: none;
		color: var(--st-ink);
	}

	.step summary::-webkit-details-marker {
		display: none;
	}

	.step.quiet summary {
		color: var(--st-muted);
	}

	.step .mark {
		flex: none;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--st-ok);
		transform: translateY(-1px);
	}

	.step.quiet .mark {
		background: var(--st-faint);
	}

	.step.running .mark {
		background: var(--st-accent);
		animation: pulse 1s ease-in-out infinite;
	}

	.step.off .mark {
		background: var(--st-fail);
	}

	.step .txt {
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.step .tail {
		flex: none;
		font-size: 11px;
		color: var(--st-faint);
	}

	.open {
		display: grid;
		gap: 3px;
		margin: 4px 0 4px 13px;
		padding: 6px 8px;
		border-radius: 4px;
		background: var(--st-edge);
	}

	.open .d {
		margin: 0;
		display: flex;
		gap: 6px;
		font-size: 11.5px;
		overflow-wrap: anywhere;
	}

	.open .sign {
		flex: none;
		font-family: var(--rb-data);
	}

	.open .was {
		color: #e6948e;
	}

	.open .is {
		color: #8fd6b1;
	}

	.open .more {
		margin: 0;
		font-size: 11px;
		color: var(--st-muted);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
	}

	.open img {
		display: block;
		max-width: 100%;
		border-radius: 3px;
	}

	.ask {
		display: grid;
		gap: 8px;
		margin-top: 8px;
		padding: 9px 10px;
		border-radius: 6px;
		background: var(--st-raised);
		box-shadow: inset 0 0 0 1px var(--st-line);
	}

	.ask .q {
		margin: 0;
		color: var(--st-ink);
	}

	.answers,
	.tools {
		display: flex;
		align-items: center;
		gap: 6px;
	}

	.grow {
		flex: 1;
	}

	.undo {
		margin-top: 6px;
	}

	.ended {
		margin: 6px 0 0;
		color: var(--st-faint);
		font-size: 11px;
	}

	.start {
		display: grid;
		align-content: end;
		gap: 8px;
		min-height: 100%;
	}

	/* Nothing asked yet: what the assistant is for in a line, and the button that starts. */
	.empty {
		display: grid;
		justify-items: center;
		gap: 8px;
		margin: auto 0 14px;
		text-align: center;
		color: var(--st-accent);
	}

	.empty p {
		max-width: 30ch;
		margin: 0;
		font-size: 12px;
		line-height: 1.4;
		color: var(--st-ink);
	}

	.or {
		margin: 0;
		font-size: 10.5px;
		color: var(--st-faint);
	}

	.firsts {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 5px;
	}

	.firsts button {
		width: 100%;
		padding: 6px 9px;
		border-radius: 6px;
		background: var(--st-raised);
		box-shadow: inset 0 0 0 1px var(--st-line);
		color: var(--st-muted);
		text-align: left;
	}

	.firsts button:hover {
		color: var(--st-ink);
		box-shadow: inset 0 0 0 1px var(--st-faint);
	}

	/* --- the box to type in --- */
	.dock {
		flex: none;
		display: grid;
		gap: 5px;
		padding: 6px 8px 8px;
		border-top: 1px solid var(--st-edge);
	}

	.status {
		display: flex;
		align-items: center;
		gap: 7px;
		padding: 0 2px;
		font-size: 11px;
		color: var(--st-muted);
	}

	.status .dot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--st-accent);
		animation: pulse 1s ease-in-out infinite;
	}

	.status .word {
		color: var(--st-ink);
	}

	.status .time {
		font-family: var(--rb-data);
		font-variant-numeric: tabular-nums;
		color: var(--st-faint);
	}

	.status .esc {
		color: var(--st-faint);
	}

	.status .esc.armed {
		color: var(--st-warn);
	}

	.composer {
		display: grid;
		gap: 6px;
		padding: 7px 8px;
		border-radius: 8px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
	}

	.composer:focus-within {
		box-shadow: inset 0 0 0 1px var(--st-accent);
	}

	.composer.off {
		opacity: 0.6;
	}

	.sel {
		justify-self: start;
		padding: 1px 7px;
		border-radius: 999px;
		background: rgba(91, 157, 240, 0.16);
		color: var(--st-accent);
		font-size: 10.5px;
	}

	textarea {
		width: 100%;
		min-height: 20px;
		padding: 0;
		border: 0;
		resize: none;
		background: none;
		color: var(--st-ink);
		font: 400 12.5px/1.45 var(--rb-text);
		outline: none;
		user-select: text;
		-webkit-user-select: text;
	}

	textarea::placeholder {
		color: var(--st-faint);
	}

	@keyframes pulse {
		50% {
			opacity: 0.35;
		}
	}

	@media (prefers-reduced-motion: reduce) {
		.step.running .mark,
		.status .dot {
			animation: none;
		}
	}
</style>
