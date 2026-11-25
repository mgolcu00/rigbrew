<script lang="ts">
	import { tick } from 'svelte';
	import { isTauri, pickFolder, type Connector } from '#lib/api.ts';
	import { commandLine, formArgs, needsLine, placeholderIn, stateWords } from '#lib/connectors.ts';
	import { connectors } from '#lib/connectors.svelte.ts';
	import { machine } from '#lib/words.ts';

	// Connectors: tools from outside Rigbrew that the person adds. The list with what runs, a
	// few ready-made ones that only fill in the form, the form itself (which shows the exact
	// command before anything is saved), and the tools of one connector with a way to try them.

	let nameField = $state<HTMLInputElement>();
	let closer = $state<HTMLButtonElement>();

	const draft = $derived(connectors.draft);
	const shown = $derived(connectors.shown);
	const placeholder = $derived(draft ? placeholderIn(draft.args) : null);
	const args = $derived(draft ? formArgs(draft.args, draft.place) : []);
	const preview = $derived(draft ? commandLine(draft.command, args) : '');
	const needs = $derived(draft?.needs ? needsLine(draft.needs, connectors.programs, machine) : null);

	$effect(() => {
		const view = connectors.sheet;
		tick().then(() => (view === 'form' ? nameField : closer)?.focus());
	});

	// While one is starting, ask again so the dot turns when it is up.
	$effect(() => {
		if (!connectors.sheet || !connectors.list.some((c) => c.state === 'starting')) return;
		const timer = setInterval(() => void connectors.load(), 1500);
		return () => clearInterval(timer);
	});

	async function save(event: SubmitEvent) {
		event.preventDefault();
		const start = (event.submitter as HTMLButtonElement | null)?.value === 'start';
		await connectors.save(start);
	}

	async function choosePlace() {
		if (!draft) return;
		const picked = await pickFolder('Choose the folder it may use');
		if (picked) draft.place = picked;
	}

	function addVariable() {
		draft?.env.push({ name: '', value: '', saved: false });
	}

	const tail = (c: Connector) => (c.kind === 'http' ? c.url.replace(/^https?:\/\//, '') : c.command_line);

	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Escape' || !connectors.sheet) return;
		event.preventDefault();
		event.stopPropagation();
		if (connectors.sheet === 'list') connectors.close();
		else connectors.back();
	}
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#if connectors.sheet}
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="veil" onclick={() => connectors.close()}>
		<div class="sheet" role="dialog" tabindex="-1" aria-modal="true" aria-label="Connectors" onclick={(e) => e.stopPropagation()}>
			{#if connectors.sheet === 'form' && draft}
				<form onsubmit={save} novalidate>
					<header>
						<h1>{draft.id ? 'Edit connector' : 'New connector'}</h1>
						<span class="grow"></span>
						<button type="button" class="btn quiet" onclick={() => connectors.back()}>Cancel</button>
						<button type="submit" class="btn" value="save" disabled={connectors.saving}>Save</button>
						<button type="submit" class="btn primary" value="start" disabled={connectors.saving}>{connectors.saving ? 'Saving…' : 'Save and start'}</button>
					</header>
					<div class="fields scroll">
						{#if !draft.id}
							<div class="chips" role="radiogroup" aria-label="What kind of connector">
								<button type="button" role="radio" aria-checked={draft.kind === 'stdio'} onclick={() => (draft.kind = 'stdio')}>A program on this {machine}</button>
								<button type="button" role="radio" aria-checked={draft.kind === 'http'} onclick={() => (draft.kind = 'http')}>A web address</button>
							</div>
						{/if}
						<label class="field">
							<span class="what">Name</span>
							<input bind:this={nameField} bind:value={draft.name} maxlength="60" placeholder="Notes" autocomplete="off" spellcheck="false" />
						</label>
						<label class="field">
							<span class="what">What it is for <span class="count">optional</span></span>
							<input bind:value={draft.description} maxlength="200" placeholder="My shopping and to-do notes" autocomplete="off" />
							<span class="hint">The model reads this line to know when to use the connector.</span>
						</label>
						{#if draft.kind === 'stdio'}
							<label class="field">
								<span class="what">Program</span>
								<input class="mono" bind:value={draft.command} placeholder="npx" autocomplete="off" spellcheck="false" autocapitalize="off" />
							</label>
							<label class="field">
								<span class="what">Arguments <span class="count">one on each line</span></span>
								<textarea class="mono" bind:value={draft.args} rows="4" spellcheck="false" autocapitalize="off" placeholder={'-y\n@modelcontextprotocol/server-memory'}></textarea>
							</label>
							{#if placeholder}
								<div class="field">
									<span class="what">{placeholder === '<file>' ? 'The file it may use' : 'The folder it may use'}</span>
									<div class="pair">
										<input class="mono" bind:value={draft.place} placeholder={placeholder === '<file>' ? '/Users/you/data.db' : '/Users/you/Documents/notes'} autocomplete="off" spellcheck="false" aria-label={placeholder === '<file>' ? 'Path of the file' : 'Path of the folder'} />
										{#if isTauri && placeholder === '<folder>'}<button type="button" class="btn" onclick={choosePlace}>Choose…</button>{/if}
									</div>
									<span class="hint">It takes the place of {placeholder} in the arguments.</span>
								</div>
							{/if}
							<fieldset class="field">
								<legend class="what">Environment variables <span class="count">optional</span></legend>
								{#each draft.env as row, i (i)}
									<div class="pair">
										<input class="mono name" bind:value={row.name} placeholder="API_KEY" autocomplete="off" spellcheck="false" autocapitalize="off" aria-label="Name of the variable" readonly={row.saved} />
										<input class="mono" type="password" bind:value={row.value} placeholder={row.saved ? 'Saved. Leave empty to keep it.' : 'Value'} autocomplete="off" aria-label="Value of {row.name || 'the variable'}" />
										<button type="button" class="btn quiet" onclick={() => draft.env.splice(i, 1)} aria-label="Remove the variable {row.name}">Remove</button>
									</div>
								{/each}
								<button type="button" class="btn add" onclick={addVariable}>Add a variable</button>
								<span class="hint">For keys and passwords the program needs. A saved value is never shown again.</span>
							</fieldset>
							<label class="field">
								<span class="what">Working folder <span class="count">optional</span></span>
								<input class="mono" bind:value={draft.cwd} placeholder="/Users/you/project" autocomplete="off" spellcheck="false" />
							</label>
							<div class="runs" role="note">
								<p>Starting this connector runs a program on your {machine}, with your own rights. Add it only if you trust where it comes from.</p>
								<p class="cap">This is the command that will run</p>
								<pre class="selectable">{preview || 'Nothing yet. Say which program to run.'}</pre>
								{#if needs}<p class:wrong={needs.missing}>{needs.text}</p>{/if}
							</div>
						{:else}
							<label class="field">
								<span class="what">Address</span>
								<input class="mono" bind:value={draft.url} placeholder="https://example.com/mcp" autocomplete="off" spellcheck="false" autocapitalize="off" inputmode="url" />
							</label>
							<label class="field">
								<span class="what">Token <span class="count">optional</span></span>
								<input class="mono" type="password" bind:value={draft.token} placeholder={draft.hasToken ? 'Saved. Leave empty to keep it.' : 'Only if the service asks for one'} autocomplete="off" />
								<span class="hint">It is sent to this address and nowhere else. A saved token is never shown again.</span>
							</label>
							<div class="runs" role="note">
								<p>What the model sends to a tool of this connector leaves your {machine} and goes to this address.</p>
							</div>
						{/if}
						{#if connectors.problem}<p class="wrong" role="alert">Could not save it. {connectors.problem}</p>{/if}
					</div>
				</form>
			{:else if connectors.sheet === 'tools' && shown}
				<header>
					<h1>{shown.name}</h1>
					<span class="grow"></span>
					<button class="btn" bind:this={closer} onclick={() => connectors.back()}>Back</button>
				</header>
				<div class="lists scroll">
					<p class="lede">These are the tools of this connector. The model asks you before it uses one, unless you tick “Always allow”. The names and descriptions are written by the connector, not by Rigbrew.</p>
					{#if connectors.problem}<p class="wrong" role="alert">{connectors.problem}</p>{/if}
					{#if connectors.tools.length === 0 && !connectors.problem}
						<p class="none">It has no tools.</p>
					{/if}
					<ul>
						{#each connectors.tools as tool (tool.name)}
							{@const trial = connectors.trial?.tool === tool.name ? connectors.trial : null}
							<li class="tool">
								<div class="row">
									<div class="about">
										<p class="aname mono">{tool.name}</p>
										<p class="does clamp">{tool.description || 'It does not say what it does.'}</p>
									</div>
									<label class="check" title="The model may use this tool without asking you first">
										<input type="checkbox" checked={tool.always} onchange={(e) => connectors.setAlways(shown.id, tool.name, e.currentTarget.checked)} />
										<span>Always allow</span>
									</label>
									<button class="btn" onclick={() => connectors.startTrial(tool)} aria-expanded={trial !== null}>Try it</button>
								</div>
								{#if trial}
									<div class="trial">
										<label class="field">
											<span class="what">Arguments <span class="count">as JSON</span></span>
											<textarea class="mono" bind:value={trial.args} rows="4" spellcheck="false" autocapitalize="off"></textarea>
										</label>
										{#if tool.params.length > 0}
											<p class="hint">It takes: {tool.params.map((p) => `${p.name} (${p.type}${p.required ? ', needed' : ''})`).join(', ')}.</p>
										{/if}
										{#if trial.problem}<p class="wrong">{trial.problem}</p>{/if}
										<div><button class="btn primary" onclick={() => connectors.runTrial()} disabled={trial.running}>{trial.running ? 'Running…' : 'Run'}</button></div>
										{#if trial.result}
											<p class="cap">{trial.ok ? 'It answered' : 'It did not work'}</p>
											<pre class="selectable" class:bad={!trial.ok}>{trial.result}</pre>
										{/if}
									</div>
								{/if}
							</li>
						{/each}
					</ul>
				</div>
			{:else}
				<header>
					<h1>Connectors</h1>
					<span class="grow"></span>
					{#if connectors.support !== 'no'}<button class="btn primary" onclick={() => connectors.startNew()}>New connector</button>{/if}
					<button class="btn" bind:this={closer} onclick={() => connectors.close()}>Close</button>
				</header>
				<div class="lists scroll">
					<p class="lede">A connector gives the model tools from outside Rigbrew: a program on this {machine}, or a web address. The model asks you before it uses a tool. Switch connectors on for a Code task, or for an agent in the chat.</p>
					{#if connectors.support === 'no'}
						<p class="wrong">This version of the background service does not have connectors yet. Quit Rigbrew and open it again.</p>
					{:else}
						{#if connectors.problem}<p class="wrong" role="alert">That did not work. {connectors.problem}</p>{/if}
						<h2>Your connectors</h2>
						{#if connectors.list.length === 0}
							<p class="none">None yet. Add one of the ready-made ones below, or make your own.</p>
						{:else}
							<ul>
								{#each connectors.list as c (c.id)}
									{@const starting = connectors.busy === c.id || c.state === 'starting'}
									<li>
										<span class="dot {starting ? 'starting' : c.state}" aria-hidden="true"></span>
										<div class="about">
											<p class="aname">{c.name}</p>
											<p class="does" class:bad={c.state === 'failed' && !starting}>{starting && c.state !== 'running' ? 'Starting. The first start can take a minute.' : stateWords(c)}</p>
											<p class="does mono clamp1" title={tail(c)}>{tail(c)}</p>
										</div>
										{#if c.state === 'running'}
											<button class="btn" onclick={() => connectors.openTools(c)}>Tools</button>
											<button class="btn quiet" onclick={() => connectors.stop(c.id)} disabled={connectors.busy !== ''}>Stop</button>
										{:else}
											<button class="btn" onclick={() => connectors.start(c.id)} disabled={connectors.busy !== '' || starting}>{c.state === 'failed' ? 'Try again' : 'Start'}</button>
										{/if}
										<button class="btn quiet" onclick={() => connectors.edit(c)}>Edit</button>
										<button class="btn quiet danger" onclick={() => connectors.remove(c.id)} disabled={connectors.busy === c.id}>Remove</button>
									</li>
								{/each}
							</ul>
						{/if}
						<h2>Ready-made</h2>
						<ul>
							{#each connectors.gallery as ready (ready.id)}
								{@const need = needsLine(ready.needs, connectors.programs, machine)}
								<li>
									<div class="about">
										<p class="aname">{ready.name}</p>
										<p class="does">{ready.does}</p>
										<p class="does" class:bad={need.missing}>{need.text}</p>
									</div>
									<button class="btn" onclick={() => connectors.startNew(ready)} title="Fills in the form. Nothing runs until you save and start it.">Add</button>
								</li>
							{/each}
						</ul>
						<p class="fine">Connectors speak the Model Context Protocol, an open standard, so a server made for it works here. The ready-made ones are published by the people behind that standard and are fetched from the internet the first time they start.</p>
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
		width: min(680px, 100%);
		max-height: min(720px, 100%);
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
		max-width: 62ch;
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

	li.tool {
		display: block;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 8px;
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
	.hint,
	.fine {
		font-size: 12.5px;
		color: var(--muted);
	}

	.fine {
		margin-top: 18px;
		max-width: 62ch;
		color: var(--faint);
	}

	.does.bad,
	pre.bad {
		color: var(--fail);
	}

	.clamp {
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		overflow: hidden;
	}

	.clamp1 {
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--faint);
	}

	.mono {
		font-family: var(--rb-data);
		font-size: 12.5px;
	}

	.none {
		padding: 8px 0 2px;
	}

	.dot {
		flex: none;
		width: 8px;
		height: 8px;
		margin-right: 8px;
		border-radius: 50%;
		background: var(--line);
	}

	.dot.running {
		background: var(--fit);
	}

	.dot.failed {
		background: var(--fail);
	}

	.dot.starting {
		background: var(--accent);
		animation: blink 1.2s steps(2, jump-none) infinite;
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

	legend.what {
		width: 100%;
	}

	.count {
		font-size: 12px;
		font-weight: 400;
		color: var(--faint);
	}

	.wrong {
		color: var(--fail);
		font-size: 12.5px;
	}

	.field input:not([type='checkbox']),
	textarea {
		width: 100%;
		min-width: 0;
		padding: 8px 10px;
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--bg);
		font-size: 14px;
		line-height: 1.5;
	}

	.field input.mono,
	textarea.mono {
		font-size: 12.5px;
	}

	textarea {
		min-height: 76px;
		resize: vertical;
	}

	.field input:focus-visible,
	textarea:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -1px;
	}

	.field input[readonly] {
		color: var(--muted);
	}

	.pair {
		display: flex;
		gap: 6px;
		align-items: center;
	}

	.pair .name {
		flex: 0 0 34%;
	}

	.btn.add {
		justify-self: start;
	}

	.chips {
		display: inline-flex;
		gap: 2px;
	}

	.chips button {
		height: 30px;
		padding: 0 12px;
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

	.runs {
		display: grid;
		gap: 6px;
		padding: 12px 14px;
		border: 1px solid var(--line);
		border-radius: 10px;
		background: var(--hover);
		font-size: 13px;
	}

	.cap {
		margin-top: 2px;
		font-size: 12px;
		color: var(--faint);
	}

	pre {
		margin: 0;
		padding: 8px 10px;
		border: 1px solid var(--line-soft);
		border-radius: 8px;
		background: var(--bg);
		font: 12.5px/1.5 var(--rb-data);
		white-space: pre-wrap;
		overflow-wrap: anywhere;
		max-height: 220px;
		overflow: auto;
	}

	.check {
		flex: none;
		display: flex;
		align-items: center;
		gap: 6px;
		font-size: 13px;
		color: var(--muted);
	}

	.check input {
		accent-color: var(--accent);
	}

	.trial {
		display: grid;
		gap: 8px;
		margin: 10px 0 6px;
		padding: 12px 14px;
		border: 1px solid var(--line-soft);
		border-radius: 10px;
	}

	@media (max-width: 620px) {
		li,
		.row {
			flex-wrap: wrap;
		}

		.about {
			flex-basis: 100%;
		}
	}
</style>
