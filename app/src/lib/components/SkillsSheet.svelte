<script lang="ts">
	import { tick } from 'svelte';
	import { isTauri, type FoundSkill, type Skill } from '#lib/api.ts';
	import { renderMarkdown } from '#lib/markdown.ts';
	import { MAX_SKILL_BODY, MAX_SKILL_DESCRIPTION, MAX_SKILL_NAME, skillName, skills } from '#lib/skills.svelte.ts';

	// Skills: every skill there is with where it comes from and a switch, a page to read one,
	// the small form that makes or changes one, and bringing skills in from a folder or from
	// GitHub, where the person sees what they get before anything is saved.

	let nameField = $state<HTMLInputElement>();
	let closer = $state<HTMLButtonElement>();
	let address = $state('');
	let folderPath = $state('');

	const draft = $derived(skills.draft);
	const reading = $derived(skills.reading);
	const bringing = $derived(skills.bringing);
	const chars = (text: string) => [...text].length;

	$effect(() => {
		const view = skills.sheet;
		tick().then(() => (view === 'form' ? nameField : closer)?.focus());
	});

	const from = (s: Skill) =>
		s.source === 'built-in' ? 'Comes with Rigbrew' : s.source === 'folder' ? 'From the folder open in Code' : s.replaces_built_in ? 'Yours, in place of the one that comes with Rigbrew' : 'Yours';
	const where = (s: Skill) => (s.where === 'code' ? 'Used in Code.' : s.where === 'chat' ? 'Used in the chat.' : 'Used in Code and in the chat.');
	const size = (bytes: number) => (bytes < 1024 ? `${bytes} bytes` : `${Math.ceil(bytes / 1024)} KB`);
	const about = (f: FoundSkill) => `${size(f.bytes)} in ${f.files.length === 1 ? '1 file' : `${f.files.length} files`}. ${f.licence ? `Licence: ${f.licence}.` : 'It names no licence.'}`;

	async function save(event: SubmitEvent) {
		event.preventDefault();
		await skills.save();
	}

	function pick(name: string, on: boolean) {
		if (!bringing) return;
		bringing.picked = on ? [...bringing.picked.filter((n) => n !== name), name] : bringing.picked.filter((n) => n !== name);
	}

	// Links in a skill open nowhere: it is read here as text.
	function onreadclick(event: MouseEvent) {
		if ((event.target as HTMLElement).closest('a')) event.preventDefault();
	}

	function onkeydown(event: KeyboardEvent) {
		if (event.key !== 'Escape' || !skills.sheet) return;
		event.preventDefault();
		event.stopPropagation();
		if (skills.sheet === 'list') skills.close();
		else skills.back();
	}
</script>

<svelte:window onkeydowncapture={onkeydown} />

{#if skills.sheet}
	<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
	<div class="veil" onclick={() => skills.close()}>
		<div class="sheet" role="dialog" tabindex="-1" aria-modal="true" aria-label="Skills" onclick={(e) => e.stopPropagation()}>
			{#if skills.sheet === 'form' && draft}
				<form onsubmit={save} novalidate>
					<header>
						<h1>{draft.was ? 'Edit skill' : 'New skill'}</h1>
						<span class="grow"></span>
						<button type="button" class="btn quiet" onclick={() => skills.back()}>Cancel</button>
						<button type="submit" class="btn primary" disabled={skills.saving}>{skills.saving ? 'Saving…' : 'Save'}</button>
					</header>
					<div class="fields scroll">
						{#if draft.builtIn}
							<p class="hint">This skill comes with Rigbrew. Saving keeps your own version in its place; deleting yours brings the original back.</p>
						{/if}
						<label class="field">
							<span class="what">Name</span>
							<input
								class="mono"
								bind:this={nameField}
								bind:value={draft.name}
								maxlength={MAX_SKILL_NAME}
								placeholder="tidy-imports"
								autocomplete="off"
								spellcheck="false"
								autocapitalize="off"
								readonly={draft.builtIn}
								onblur={() => (draft.name = skillName(draft.name))}
							/>
							<span class="hint">Lowercase letters, digits and dashes. The model calls the skill by this name.</span>
						</label>
						<label class="field">
							<span class="what">When to use it <span class="count num" class:over={chars(draft.description.trim()) > MAX_SKILL_DESCRIPTION}>{chars(draft.description.trim())} of {MAX_SKILL_DESCRIPTION}</span></span>
							<textarea class="short" bind:value={draft.description} rows="2" placeholder="Sort the imports of a file. Use when asked to tidy or order imports."></textarea>
							<span class="hint">The model reads only this line until it decides to use the skill. Say what it does and when it fits.</span>
						</label>
						<label class="field">
							<span class="what">Instructions <span class="count num" class:over={chars(draft.body) > MAX_SKILL_BODY}>{chars(draft.body)} of {MAX_SKILL_BODY}</span></span>
							<textarea class="mono" bind:value={draft.body} rows="14" spellcheck="false" placeholder={'# Steps\n\n1. Read the file.\n2. …\n\n## The answer\n\nSay what you changed.'}></textarea>
							<span class="hint">Steps, checks and what to answer. Short works best: a small model reads the first 6,000 characters.</span>
						</label>
						{#if skills.problem}<p class="wrong" role="alert">Could not save it. {skills.problem}</p>{/if}
					</div>
				</form>
			{:else if skills.sheet === 'read' && reading}
				<header>
					<h1 class="mono">{reading.name}</h1>
					<span class="grow"></span>
					{#if reading.source !== 'folder'}<button class="btn quiet" onclick={() => skills.edit(reading)}>Edit</button>{/if}
					<button class="btn quiet" onclick={() => skills.duplicate(reading)}>Duplicate</button>
					<button class="btn" bind:this={closer} onclick={() => skills.back()}>Back</button>
				</header>
				<div class="lists scroll">
					<p class="lede">{reading.description}</p>
					<p class="fine">{from(reading)}. {where(reading)}{reading.licence ? ` Licence: ${reading.licence}.` : ''}</p>
					<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
					<div class="prose selectable body" onclick={onreadclick}>{@html renderMarkdown(reading.body, false)}</div>
					{#if reading.files.length > 0}
						<h2>Other files of this skill</h2>
						<ul class="files">
							{#each reading.files as file (file.path)}
								<li><span class="mono">{file.path}</span><span class="does">{size(file.bytes)}</span></li>
							{/each}
						</ul>
					{/if}
				</div>
			{:else if skills.sheet === 'import' && bringing}
				<header>
					<h1>Add skills from outside</h1>
					<span class="grow"></span>
					{#if bringing.found.length > 0}
						<button class="btn primary" onclick={() => skills.bringIn()} disabled={bringing.saving || bringing.picked.length === 0}>
							{bringing.saving ? 'Adding…' : bringing.picked.length === 1 ? 'Add 1 skill' : `Add ${bringing.picked.length} skills`}
						</button>
					{/if}
					<button class="btn" bind:this={closer} onclick={() => skills.back()}>Back</button>
				</header>
				<div class="lists scroll">
					<p class="lede">A skill is a folder with a SKILL.md file and, sometimes, a few more text files. You see what each one is before it is added. Only text is taken, 200 KB at most for one skill, and nothing in it is ever run.</p>
					<form class="field" onsubmit={(e) => (e.preventDefault(), skills.look('github', address))}>
						<span class="what">From GitHub</span>
						<div class="pair">
							<input class="mono" bind:value={address} placeholder="https://github.com/owner/repository/tree/main/skills" autocomplete="off" spellcheck="false" autocapitalize="off" inputmode="url" aria-label="Address of a skill folder, or of a repository of skills, on GitHub" />
							<button class="btn" type="submit" disabled={bringing.looking || !address.trim()}>Look</button>
						</div>
						<span class="hint">The address of one skill folder, or of a public repository that holds several. The address is sent to GitHub.</span>
					</form>
					<div class="field">
						<span class="what">From a folder on this computer</span>
						{#if isTauri}
							<div><button class="btn" onclick={() => skills.chooseFolder()} disabled={bringing.looking}>Choose a folder…</button></div>
						{:else}
							<form class="pair" onsubmit={(e) => (e.preventDefault(), skills.look('folder', folderPath))}>
								<input class="mono" bind:value={folderPath} placeholder="/path/to/a/skill" autocomplete="off" spellcheck="false" aria-label="Path of the folder" />
								<button class="btn" type="submit" disabled={bringing.looking || !folderPath.trim()}>Look</button>
							</form>
						{/if}
					</div>
					{#if bringing.looking}<p class="none"><span class="spot"></span>Looking…</p>{/if}
					{#if bringing.problem}<p class="wrong" role="alert">{bringing.problem}</p>{/if}
					{#if bringing.found.length > 0}
						<h2>Found {bringing.found.length === 1 ? '1 skill' : `${bringing.found.length} skills`}</h2>
						<ul>
							{#each bringing.found as f (f.at + f.name)}
								<li class="found">
									<input type="checkbox" checked={bringing.picked.includes(f.name)} disabled={Boolean(f.problem)} onchange={(e) => pick(f.name, e.currentTarget.checked)} aria-label="Add the skill {f.name || f.at}" />
									<div class="about">
										<p class="aname mono">{f.name || f.at || 'No name'}</p>
										{#if f.description}<p class="does">{f.description}</p>{/if}
										<p class="does">{about(f)}{f.replaces_built_in && !f.problem ? ' It will take the place of the skill with this name that comes with Rigbrew.' : ''}</p>
										{#if f.problem}<p class="wrong">It cannot be added. {f.problem}</p>{/if}
									</div>
								</li>
							{/each}
						</ul>
						<p class="fine">A skill is instructions the model follows. Read one after adding it, and switch it off if you do not like what it says.</p>
					{/if}
				</div>
			{:else}
				<header>
					<h1>Skills</h1>
					<span class="grow"></span>
					{#if skills.support !== 'no'}
						<button class="btn" onclick={() => skills.startImport()}>Import</button>
						<button class="btn primary" onclick={() => skills.startNew()}>New skill</button>
					{/if}
					<button class="btn" bind:this={closer} onclick={() => skills.close()}>Close</button>
				</header>
				<div class="lists scroll">
					<p class="lede">A skill is a short set of instructions for one kind of work. The model sees the name and when to use it, and reads the instructions when your request fits. Switch off the ones you do not want.</p>
					{#if skills.support === 'no'}
						<p class="wrong">This version of the background service cannot manage skills yet. Quit Rigbrew and open it again.</p>
					{:else}
						{#if skills.problem}<p class="wrong" role="alert">That did not work. {skills.problem}</p>{/if}
						<ul class="all">
							{#each skills.list as s (s.name)}
								<li class:off={!s.enabled}>
									<label class="switch" title={s.enabled ? 'Switch this skill off' : 'Switch this skill on'}>
										<input type="checkbox" checked={s.enabled} onchange={(e) => skills.setEnabled(s, e.currentTarget.checked)} aria-label="Use the skill {s.name}" />
									</label>
									<div class="about">
										<p class="aname mono">{s.name}</p>
										<p class="does clamp">{s.description}</p>
										<p class="does src">{from(s)}. {where(s)}</p>
									</div>
									<button class="btn" onclick={() => skills.read(s)}>Open</button>
									{#if s.source !== 'folder'}<button class="btn quiet" onclick={() => skills.edit(s)}>Edit</button>{/if}
									<button class="btn quiet" onclick={() => skills.duplicate(s)}>Duplicate</button>
									{#if s.source === 'yours'}<button class="btn quiet danger" onclick={() => skills.remove(s)}>Delete</button>{/if}
								</li>
							{/each}
						</ul>
						{#if skills.dir}<p class="fine selectable">Your own skills are folders in {skills.dir}. A project can carry its own in .rigbrew/skills; Code uses those for that folder.</p>{/if}
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
		width: min(720px, 100%);
		max-height: min(740px, 100%);
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

	form:not(.field):not(.pair) {
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
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 600 15px/1.3 var(--rb-text);
		letter-spacing: 0;
	}

	h1.mono {
		font-family: var(--rb-data);
		font-size: 14px;
	}

	.grow {
		flex: 1;
	}

	.lists,
	.fields {
		min-height: 0;
		padding: 16px 20px 22px;
	}

	.lists > .field {
		margin-top: 16px;
	}

	.lede {
		max-width: 64ch;
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

	ul.all {
		margin-top: 14px;
	}

	li {
		display: flex;
		align-items: center;
		gap: 4px;
		min-height: 52px;
		padding: 9px 0;
		border-top: 1px solid var(--line-soft);
	}

	li.off .about {
		opacity: 0.55;
	}

	li.found {
		align-items: flex-start;
		gap: 10px;
	}

	li.found input {
		margin-top: 4px;
		accent-color: var(--accent);
	}

	.files li {
		min-height: 0;
		justify-content: space-between;
		gap: 12px;
		padding: 6px 0;
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
		font-size: 13px;
		font-weight: 600;
	}

	.mono {
		font-family: var(--rb-data);
	}

	.does,
	.none,
	.hint,
	.fine {
		font-size: 12.5px;
		color: var(--muted);
	}

	.does.src,
	.fine {
		color: var(--faint);
	}

	.fine {
		margin-top: 10px;
		max-width: 64ch;
	}

	.clamp {
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 2;
		line-clamp: 2;
		overflow: hidden;
	}

	.none {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 12px 0 2px;
	}

	.spot {
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--accent);
		animation: blink 1.2s steps(2, jump-none) infinite;
	}

	.body {
		margin-top: 14px;
		padding-top: 14px;
		border-top: 1px solid var(--line-soft);
		font-size: 14px;
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
	.pair input,
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
	.pair input.mono,
	textarea.mono {
		font-size: 12.5px;
	}

	textarea {
		min-height: 200px;
		resize: vertical;
	}

	textarea.short {
		min-height: 56px;
	}

	.field input:focus-visible,
	.pair input:focus-visible,
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

	.switch {
		flex: none;
		display: inline-flex;
		align-items: center;
		margin-right: 10px;
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

	.switch input:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 2px;
	}

	@media (max-width: 640px) {
		li:not(.found) {
			flex-wrap: wrap;
		}

		li:not(.found) .about {
			flex-basis: calc(100% - 50px);
		}
	}
</style>
