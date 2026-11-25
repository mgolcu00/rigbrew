<script lang="ts">
	import { tick } from 'svelte';
	import { MAX_INSTRUCTION, MAX_NAME, MAX_STARTER, abilities, chars, checkAgent, examples, type Agent } from '#lib/agents.ts';
	import { agents } from '#lib/agents.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { connectors } from '#lib/connectors.svelte.ts';
	import { skills } from '#lib/skills.svelte.ts';
	import { app } from '#lib/state.svelte.ts';

	// Agents: the list of the ones the person made and three examples to copy, and the small
	// form that makes or changes one. A sheet over the app, like Settings and Help.

	let nameField = $state<HTMLInputElement>();
	let closer = $state<HTMLButtonElement>();
	/** Problems are said once Save was tried, not while the form is still empty. */
	let tried = $state(false);

	const draft = $derived(agents.draft);
	const problems = $derived(draft ? checkAgent({ ...draft, id: draft.saved ? draft.id : 'new' }) : {});
	const summary = (agent: Agent) => {
		const can = abilities.filter((a) => agent.tools.includes(a.tool)).map((a) => a.label.toLowerCase());
		if (agent.connectors) can.push('use connectors');
		const pinned = agent.skills?.length ? ` ${agent.skills.length === 1 ? '1 skill' : `${agent.skills.length} skills`} pinned.` : '';
		return (can.length ? `Can ${can.join(', ')}.` : 'Answers in words only.') + pinned;
	};
	/** The skills an agent can be given: the ones that are on and not only for Code. */
	const pinnable = $derived(skills.list.filter((s) => s.enabled && s.where !== 'code'));

	$effect(() => {
		const form = agents.sheet === 'form';
		tried = false;
		tick().then(() => (form ? nameField : closer)?.focus());
	});

	function use(agent: Agent) {
		chat.newChat();
		chat.setAgent(agent.id, 'sheet');
		app.go('chat');
		agents.close();
		chat.focusRequest++;
	}

	async function copy(source: Agent) {
		const made = await agents.copy(source, true);
		if (made) agents.edit(made);
	}

	async function save(event: SubmitEvent) {
		event.preventDefault();
		tried = true;
		if (Object.keys(problems).length > 0) return;
		await agents.save();
	}

	function pin(name: string, on: boolean) {
		if (!draft) return;
		const now = (draft.skills ?? []).filter((n) => n !== name);
		draft.skills = on ? [...now, name] : now;
	}

	function toggle(tool: Agent['tools'][number], on: boolean) {
		if (!draft) return;
		draft.tools = on ? [...draft.tools.filter((t) => t !== tool), tool] : draft.tools.filter((t) => t !== tool);
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Escape' || !agents.sheet) return;
		event.preventDefault();
		event.stopPropagation();
		if (agents.sheet === 'form') agents.cancel();
		else agents.close();
	}
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#if agents.sheet}
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="veil" onclick={() => agents.close()}>
		<div class="sheet" role="dialog" tabindex="-1" aria-modal="true" aria-label="Agents" onclick={(e) => e.stopPropagation()}>
			{#if agents.sheet === 'form' && draft}
				<form onsubmit={save} novalidate>
					<header>
						<h1>{draft.saved ? 'Edit agent' : 'New agent'}</h1>
						<span class="grow"></span>
						<button type="button" class="btn quiet" onclick={() => agents.cancel()}>Cancel</button>
						<button type="submit" class="btn primary" disabled={agents.saving}>{agents.saving ? 'Saving…' : 'Save'}</button>
					</header>
					<div class="fields scroll">
						<label class="field">
							<span class="what">Name</span>
							<input bind:this={nameField} bind:value={draft.name} maxlength={MAX_NAME} placeholder="Editor" autocomplete="off" spellcheck="false" aria-invalid={tried && Boolean(problems.name)} />
							{#if tried && problems.name}<span class="wrong">{problems.name}</span>{/if}
						</label>
						<label class="field">
							<span class="what">Instruction <span class="count num" class:over={chars(draft.instruction.trim()) > MAX_INSTRUCTION}>{chars(draft.instruction.trim())} of {MAX_INSTRUCTION}</span></span>
							<textarea bind:value={draft.instruction} rows="9" placeholder="Say what it should do and how it should answer, in your own words." aria-invalid={tried && Boolean(problems.instruction)}></textarea>
							{#if tried && problems.instruction}<span class="wrong">{problems.instruction}</span>{:else}<span class="hint">The chat model reads this before every message in a chat that uses this agent.</span>{/if}
						</label>
						<fieldset class="field">
							<legend class="what">It may</legend>
							{#each abilities as a (a.tool)}
								<label class="check" title={a.does}>
									<input type="checkbox" checked={draft.tools.includes(a.tool)} onchange={(e) => toggle(a.tool, e.currentTarget.checked)} />
									<span>{a.label}</span>
								</label>
							{/each}
							<span class="hint">Each one needs its model installed. With none ticked it answers in words only.</span>
							<label class="check" title="It may use the tools of the connectors you added and started">
								<input type="checkbox" checked={draft.connectors === true} onchange={(e) => (draft.connectors = e.currentTarget.checked)} />
								<span>Use connectors</span>
							</label>
							<span class="hint">
								{#if connectors.list.length === 0}
									You have no connectors yet. <button type="button" class="link" onclick={() => connectors.open('agent')}>Add one</button> to give it tools from outside.
								{:else}
									It may use the tools of your running connectors. You are asked before each use.
								{/if}
							</span>
						</fieldset>
						{#if pinnable.length > 0}
							<fieldset class="field">
								<legend class="what">Skills <span class="count">optional</span></legend>
								<div class="pins">
									{#each pinnable as s (s.name)}
										<label class="check" title={s.description}>
											<input type="checkbox" checked={draft.skills?.includes(s.name) ?? false} onchange={(e) => pin(s.name, e.currentTarget.checked)} />
											<span>{s.name}</span>
										</label>
									{/each}
								</div>
								<span class="hint">Tick the skills this agent should have at hand. With none ticked it is offered every skill that is switched on.</span>
							</fieldset>
						{/if}
						<label class="field">
							<span class="what">First message to suggest <span class="count">optional</span></span>
							<input bind:value={draft.starter} maxlength={MAX_STARTER} placeholder="Tighten this text:" autocomplete="off" />
							{#if tried && problems.starter}<span class="wrong">{problems.starter}</span>{/if}
						</label>
						{#if agents.problem}<p class="wrong" role="alert">Could not save it. {agents.problem}</p>{/if}
					</div>
				</form>
			{:else}
				<header>
					<h1>Agents</h1>
					<span class="grow"></span>
					{#if agents.support !== 'no'}<button class="btn primary" onclick={() => agents.startNew('sheet')}>New agent</button>{/if}
					<button class="btn" bind:this={closer} onclick={() => agents.close()}>Close</button>
				</header>
				<div class="lists scroll">
					<p class="lede">An agent is a saved way to use the chat model: a name, an instruction, and what it may do. Pick one for a chat in the box where you type.</p>
					{#if agents.support === 'no'}
						<p class="wrong">This version of the background service cannot keep agents yet. Update Rigbrew to make your own.</p>
					{:else}
						{#if agents.problem}<p class="wrong" role="alert">That did not work. {agents.problem}</p>{/if}
						<h2>Your agents</h2>
						{#if agents.list.length === 0}
							<p class="none">None yet. Make one, or copy an example below and change it.</p>
						{:else}
							<ul>
								{#each agents.list as agent (agent.id)}
									<li>
										<div class="about">
											<p class="aname">{agent.name}</p>
											<p class="does">{summary(agent)}</p>
										</div>
										<button class="btn" onclick={() => use(agent)} disabled={chat.busy} title="Start a new chat with {agent.name}">Use</button>
										<button class="btn quiet" onclick={() => agents.edit(agent)}>Edit</button>
										<button class="btn quiet" onclick={() => agents.copy(agent)} disabled={agents.saving}>Duplicate</button>
										<button class="btn quiet danger" onclick={() => agents.remove(agent.id)}>Delete</button>
									</li>
								{/each}
							</ul>
						{/if}
						<h2>Examples</h2>
						<ul>
							{#each examples as example (example.id)}
								<li>
									<div class="about">
										<p class="aname">{example.name}</p>
										<p class="does">{example.does}</p>
									</div>
									<button class="btn" onclick={() => copy(example)} disabled={agents.saving} title="Make your own copy of {example.name} and change it">Copy</button>
								</li>
							{/each}
						</ul>
					{/if}
				</div>
			{/if}
		</div>
	</div>
{/if}

<style>
	.veil {
		position: fixed;
		inset: 0;
		z-index: 60;
		display: flex;
		justify-content: center;
		align-items: flex-start;
		padding: 56px 16px 16px;
		background: color-mix(in srgb, var(--rb-board) 62%, transparent);
	}

	.sheet {
		width: min(620px, 100%);
		max-height: min(680px, 100%);
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 12px;
		background: var(--bg);
		box-shadow: 0 18px 50px -14px var(--shade);
		overflow: hidden;
		font-family: var(--rb-text);
		animation: appear 0.14s var(--ease) both;
	}

	form {
		min-height: 0;
		display: flex;
		flex-direction: column;
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 6px;
		min-height: 48px;
		padding: 0 12px 0 20px;
		border-bottom: 1px solid var(--line-soft);
	}

	h1 {
		font: 600 15px/1.3 var(--rb-text);
		letter-spacing: 0;
	}

	.grow {
		flex: 1;
	}

	.lists,
	.fields {
		min-height: 0;
		padding: 16px 20px 22px;
	}

	.lede {
		max-width: 60ch;
		color: var(--muted);
		font-size: 13.5px;
	}

	h2 {
		margin: 20px 0 6px;
		font: 600 12px/1.3 var(--rb-text);
		letter-spacing: 0;
		color: var(--faint);
	}

	ul {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	li {
		display: flex;
		align-items: center;
		gap: 4px;
		min-height: 52px;
		padding: 8px 0;
		border-top: 1px solid var(--line-soft);
	}

	.about {
		flex: 1;
		min-width: 0;
		padding-right: 8px;
	}

	.aname {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 14px;
		font-weight: 600;
	}

	.does,
	.none,
	.hint {
		font-size: 12.5px;
		color: var(--muted);
	}

	.none {
		padding: 8px 0 2px;
	}

	.fields {
		display: grid;
		gap: 16px;
	}

	.field {
		display: grid;
		gap: 6px;
		min-width: 0;
		margin: 0;
		padding: 0;
		border: 0;
	}

	.what {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		gap: 12px;
		padding: 0;
		font-size: 13px;
		font-weight: 600;
	}

	.count {
		font-size: 12px;
		font-weight: 400;
		color: var(--faint);
	}

	.count.over,
	.wrong {
		color: var(--fail);
	}

	.wrong {
		font-size: 12.5px;
	}

	.field input:not([type='checkbox']),
	textarea {
		width: 100%;
		padding: 8px 10px;
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--bg);
		font-size: 14px;
		line-height: 1.5;
	}

	textarea {
		min-height: 120px;
		resize: vertical;
	}

	.field input:focus-visible,
	textarea:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -1px;
	}

	[aria-invalid='true'] {
		border-color: var(--fail) !important;
	}

	.check {
		display: flex;
		align-items: center;
		gap: 8px;
		min-height: 26px;
		font-size: 14px;
	}

	.check input {
		accent-color: var(--accent);
	}

	.pins {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(190px, 1fr));
		gap: 0 12px;
	}

	.pins span {
		font-family: var(--rb-data);
		font-size: 12.5px;
	}

	@media (max-width: 620px) {
		li {
			flex-wrap: wrap;
		}

		.about {
			flex-basis: 100%;
		}
	}
</style>
