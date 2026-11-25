<script lang="ts">
	import { tick } from 'svelte';
	import * as api from '#lib/api.ts';
	import { app } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { plain } from '#lib/errors.ts';
	import { bytes, size } from '#lib/format.ts';
	import { machine, usesMemory } from '#lib/words.ts';
	import type { CommunityFound, Package } from '#lib/types.ts';
	import ErrorLine from './ErrorLine.svelte';
	import Icon from './Icon.svelte';
	import InstallControl from './InstallControl.svelte';

	// Community models: open chat models a person adds by pasting a link to Hugging Face or
	// GitHub. They sit under Rigbrew's own models and every one of them says, in a full
	// sentence, that Rigbrew has not tested it, who it comes from and under which licence.

	let link = $state('');
	let looking = $state(false);
	let problem = $state('');
	let found = $state<CommunityFound | null>(null);
	/** The link the result on screen belongs to: what Install sends back. */
	let asked = $state('');
	let chosen = $state('');
	let adding = $state(false);
	let removing = $state<Record<string, 'ask' | 'busy'>>({});
	let list = $state<HTMLElement | null>(null);

	const models = $derived(app.communityPackages);
	const file = $derived(found?.files.find((f) => f.file === chosen) ?? null);

	/** The service writes its own sentences for links it refuses and repos it cannot find. */
	const say = (e: unknown) => (e instanceof api.ApiError && [400, 404, 409, 502].includes(e.status) ? e.message : plain(e).text);

	async function lookUp(what = link) {
		const url = what.trim();
		if (!url || looking) return;
		link = url;
		looking = true;
		problem = '';
		found = null;
		try {
			const answer = await api.communityLookup(url);
			found = answer;
			asked = url;
			chosen = answer.pick ?? '';
			// The suggested file is often far down a long list.
			tick().then(() => list?.querySelector('input:checked')?.scrollIntoView({ block: 'nearest' }));
		} catch (e) {
			problem = say(e);
		} finally {
			looking = false;
		}
	}

	async function install() {
		if (!found || !file || adding) return;
		adding = true;
		problem = '';
		try {
			const id = await app.addCommunity(asked, file.file, file.size);
			found = null;
			link = '';
			queueMicrotask(() => document.getElementById(`model-${id}`)?.scrollIntoView({ block: 'nearest' }));
		} catch (e) {
			problem = say(e);
		} finally {
			adding = false;
		}
	}

	async function remove(pkg: Package) {
		removing[pkg.id] = 'busy';
		try {
			const freed = await app.removeCommunity(pkg.id);
			app.say(freed > 0 ? `Removed ${pkg.name}. Freed ${bytes(freed)}.` : `Removed ${pkg.name}.`);
		} catch (e) {
			app.say(`Could not remove ${pkg.name}. ${say(e)}`);
		} finally {
			delete removing[pkg.id];
		}
	}

	function use(pkg: Package) {
		chat.model = pkg.id;
		chat.newChat();
		chat.setMode('chat', 'click');
		app.go('chat');
	}

	const siteOf = (url: string) => (url.startsWith('https://github.com/') ? 'GitHub' : 'Hugging Face');
	const ownerOf = (pkg: Package) => pkg.source.name.split('/')[0];
	const stated = (licence: string | null | undefined) => Boolean(licence) && licence !== 'no licence stated';

	/** "It comes from bartowski on Hugging Face under apache-2.0." */
	function origin(owner: string, site: string, licence: string | null | undefined) {
		return stated(licence) ? `It comes from ${owner} on ${site} under ${licence}.` : `It comes from ${owner} on ${site}. No licence is stated.`;
	}
</script>

<section class="community" aria-labelledby="community-title">
	<h2 id="community-title">Community</h2>
	<p class="intro">
		Add an open chat model by pasting its link from Hugging Face or GitHub. Rigbrew has not tested these models, so check who made one before you install it.
	</p>

	<form
		class="paste"
		onsubmit={(e) => {
			e.preventDefault();
			lookUp();
		}}
	>
		<input
			type="text"
			bind:value={link}
			placeholder="https://huggingface.co/owner/model"
			aria-label="Link to a model on Hugging Face or GitHub"
			autocomplete="off"
			autocapitalize="off"
			spellcheck="false"
			maxlength="500"
		/>
		<button class="btn" type="submit" disabled={looking || !link.trim()}>{looking ? 'Looking…' : 'Look up'}</button>
	</form>
	{#if problem}<p class="problem selectable" role="alert">{problem}</p>{/if}

	{#if found}
		{@const where = `${found.owner}/${found.repo}`}
		<div class="found" aria-live="polite">
			<p class="from">
				<b class="selectable">{where}</b>
				<span class="sep">{found.site}</span>
				<span class="sep">{stated(found.license) ? `Licence: ${found.license}` : 'No licence stated'}</span>
				<button class="link page" onclick={() => api.openExternal(found!.url)}>Open the page</button>
			</p>
			{#if found.files.length === 0}
				{#if found.links.length > 0}
					<p>This project has no model files of its own. Its README links to these models on Hugging Face:</p>
					<p class="links">
						{#each found.links as id (id)}
							<button class="btn" onclick={() => lookUp(id)} disabled={looking}>{id}</button>
						{/each}
					</p>
				{:else if found.host === 'github'}
					<p>No chat model files were found in this project’s latest release, and its README links to no model on Hugging Face.</p>
				{:else}
					<p>No chat model files in GGUF format were found there. Look for a version of the model with GGUF in its name.</p>
				{/if}
			{:else if found.gated}
				<p>{found.owner} asks people to sign in on {found.site} and agree to its terms before downloading. Rigbrew cannot do that.</p>
			{:else}
				<ul class="files" role="radiogroup" aria-label="Files of {where}" bind:this={list}>
					{#each found.files as f (f.file)}
						<li class:unfit={!f.fits}>
							<label>
								<input type="radio" name="community-file" value={f.file} bind:group={chosen} disabled={!f.fits} />
								<span class="fname num selectable">{f.file}</span>
								<span class="num fsize">{bytes(f.size)}</span>
								{#if f.installed}
									<span class="tag accent">installed</span>
								{:else if !f.fits}
									<span class="no">Too big for this {machine}</span>
								{:else if f.file === found.pick}
									<span class="tag fit" title="The largest file that fits this {machine}’s memory">suggested</span>
								{:else}
									<span class="tag fit">fits</span>
								{/if}
							</label>
						</li>
					{/each}
				</ul>
				<p class="note">
					Models may use {size(found.budget_mb)} of this {machine}’s memory. A file needs its own size and about 15% more. Larger files of the same model
					answer better.{#if found.skipped > 0}&nbsp;{found.skipped === 1 ? 'One file was' : `${found.skipped} files were`} left out: split models and picture add-ons do not work yet.{/if}
				</p>
				{#if !found.pick}
					<p class="no">None of these files fits this {machine}’s memory.</p>
				{/if}
				<p class="untested"><Icon name="warn" size={14} /> Not tested by Rigbrew. {origin(found.owner, found.site, found.license)}</p>
				{#if file && !file.checked}
					<p class="note">{found.site} gives no checksum for this file, so Rigbrew can only check its size after the download.</p>
				{/if}
				<p class="go">
					<button class="btn primary" onclick={install} disabled={!file || adding || file.installed}>
						<Icon name="download" size={14} />
						{adding ? 'Starting…' : 'Install'}
						{#if file}<span class="num weight">{bytes(file.size)}</span>{/if}
					</button>
					{#if file}<span class="muted">{usesMemory(file.needs_mb)}</span>{/if}
					<button class="btn quiet" onclick={() => (found = null)}>Close</button>
				</p>
			{/if}
		</div>
	{/if}

	{#if models.length > 0}
		<ul class="models">
			{#each models as pkg (pkg.id)}
				{@const v = pkg.variants[0]}
				{@const installed = pkg.installed.length > 0}
				{@const loaded = chat.engine?.package === pkg.id}
				{@const site = siteOf(pkg.source.url)}
				<li id="model-{pkg.id}" class:installed>
					<span class="ico"><Icon name="chat" size={16} /></span>
					<div class="what">
						<p class="name">
							<span class="pname selectable">{pkg.name}</span>
							<span class="tag">community</span>
							{#if installed}<span class="tag accent">{loaded ? 'ready' : 'installed'}</span>{/if}
						</p>
						<p class="does">
							Answers questions, writes and explains. Not tested by Rigbrew. {origin(ownerOf(pkg), site, pkg.source.license)}
							<button class="link" onclick={() => api.openExternal(pkg.source.url)}>Open the page</button>
						</p>
						<p class="facts">
							{#if v}<span>{usesMemory(v.accel_mem_mb)}</span><span class="sep">{installed ? bytes(app.diskOf(pkg, v.id)) : size(v.disk_mb)} on disk</span>{/if}
						</p>
						{#if app.installs[pkg.id]?.state === 'failed'}
							<ErrorLine error={app.installs[pkg.id].error} lead="The download did not finish." where="install" />
						{/if}
					</div>
					<div class="act">
						{#if removing[pkg.id]}
							<span class="ask" role="group" aria-label="Remove {pkg.name}">
								<span class="muted">{installed ? 'Deletes the file from this disk' : 'Takes it off this list'}</span>
								<button class="btn danger" onclick={() => remove(pkg)} disabled={removing[pkg.id] === 'busy'}>{removing[pkg.id] === 'busy' ? 'Removing…' : 'Remove'}</button>
								<button class="btn quiet" onclick={() => delete removing[pkg.id]} disabled={removing[pkg.id] === 'busy'}>Keep</button>
							</span>
						{:else}
							{#if installed}
								<button class="btn" onclick={() => use(pkg)}><Icon name="chat" size={14} /> Open a chat</button>
							{:else}
								<InstallControl {pkg} variant={v?.id} quiet />
							{/if}
							{#if app.installs[pkg.id]?.state !== 'running'}
								<button class="btn quiet" onclick={() => (removing[pkg.id] = 'ask')} disabled={chat.busy} title="Remove {pkg.name}">
									<Icon name="trash" size={14} />
									Remove
								</button>
							{/if}
						{/if}
					</div>
				</li>
			{/each}
		</ul>
	{/if}
</section>

<style>
	.community {
		max-width: 1080px;
		margin-top: 28px;
	}

	h2 {
		font: 600 14px/1.2 var(--rb-display);
		letter-spacing: -0.045em;
	}

	.intro {
		margin-top: 4px;
		max-width: 72ch;
		color: var(--muted);
	}

	.paste {
		display: flex;
		gap: 6px;
		max-width: 620px;
		margin-top: 10px;
	}

	.paste input {
		flex: 1;
		min-width: 0;
		height: 24px;
		padding: 0 8px;
		border: 0;
		border-radius: var(--r);
		box-shadow: inset 0 0 0 1px var(--line);
		background: var(--bg);
		font-size: 12.5px;
	}

	.paste input:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: -1px;
	}

	.problem {
		margin-top: 8px;
		max-width: 72ch;
	}

	.found {
		max-width: 760px;
		margin-top: 12px;
		padding: 10px 12px 12px;
		border: 1px solid var(--line);
		border-radius: 6px;
		background: var(--well);
	}

	.found > p {
		max-width: 72ch;
	}

	.from {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 2px 0;
		margin-bottom: 8px;
		color: var(--muted);
	}

	.from b {
		font: 600 13px/1.2 var(--rb-display);
		letter-spacing: -0.045em;
		color: var(--ink);
	}

	.sep::before {
		content: '·';
		margin: 0 7px;
		color: var(--faint);
	}

	.page {
		margin-left: auto;
		padding-left: 12px;
	}

	.links {
		display: flex;
		flex-wrap: wrap;
		gap: 6px;
		margin-top: 8px;
	}

	.files {
		max-height: 236px;
		overflow-y: auto;
		border-top: 1px solid var(--line-soft);
		background: var(--bg);
	}

	.files label {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 28px;
		padding: 0 8px;
		border-bottom: 1px solid var(--line-soft);
		font-size: 12.5px;
	}

	.files label:hover {
		background: var(--hover);
	}

	.files input {
		flex: none;
		margin: 0;
		accent-color: var(--accent);
	}

	.fname {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12px;
	}

	.fsize {
		flex: none;
		font-size: 12px;
		color: var(--muted);
	}

	.unfit .fname,
	.unfit .fsize {
		color: var(--faint);
	}

	.no {
		color: var(--fail);
		font-size: 12.5px;
	}

	.note {
		margin-top: 8px;
		font-size: 12px;
		color: var(--muted);
	}

	.untested {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-top: 10px;
		color: var(--ink);
	}

	.go {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 8px;
		margin-top: 10px;
		font-size: 12.5px;
	}

	.weight {
		font-size: 11.5px;
		font-weight: 400;
		opacity: 0.8;
	}

	/* The added models: the same rows as Rigbrew's own list. */
	.models {
		margin-top: 14px;
		border-top: 1px solid var(--line);
	}

	.models > li {
		display: grid;
		grid-template-columns: 32px minmax(0, 1fr) auto;
		align-items: start;
		gap: 4px 12px;
		padding: 12px 0;
		border-bottom: 1px solid var(--line-soft);
	}

	.ico {
		display: grid;
		place-items: center;
		width: 30px;
		height: 30px;
		border-radius: var(--r);
		background: var(--well);
		box-shadow: inset 0 0 0 1px var(--line-soft);
	}

	.what {
		min-width: 0;
	}

	.name {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		gap: 4px 8px;
		min-height: 20px;
	}

	.pname {
		font: 600 14px/1.2 var(--rb-display);
		letter-spacing: -0.045em;
		overflow-wrap: anywhere;
	}

	.does {
		margin-top: 2px;
		max-width: 72ch;
	}

	.facts {
		display: flex;
		align-items: center;
		flex-wrap: wrap;
		margin-top: 3px;
		font-size: 12.5px;
		color: var(--muted);
	}

	.act {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		flex-wrap: wrap;
		gap: 6px;
		padding-top: 2px;
	}

	.ask {
		display: inline-flex;
		align-items: center;
		flex-wrap: wrap;
		justify-content: flex-end;
		gap: 6px;
		font-size: 12px;
	}

	@media (max-width: 760px) {
		.models > li {
			grid-template-columns: 32px minmax(0, 1fr);
		}

		.act {
			grid-column: 2;
			justify-content: flex-start;
		}
	}
</style>
