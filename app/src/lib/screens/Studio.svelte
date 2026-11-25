<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import * as api from '#lib/api.ts';
	import type { StudioListing, StudioShot, StudioView } from '#lib/api.ts';
	import { videoLengthsOf, videoSizesOf, type SizeName } from '#lib/chat.svelte.ts';
	import { plain } from '#lib/errors.ts';
	import { app } from '#lib/state.svelte.ts';
	import Icon from '#lib/components/Icon.svelte';

	// Studio: a film is a list of shots in a folder. Each shot is one description and one clip;
	// the video model makes them one after another, so a long film renders on its own. The chat
	// model can write the shots from an idea. Clips stay in the folder as plain files.

	let films = $state<StudioListing[]>([]);
	let view = $state<StudioView | null>(null);
	let problem = $state('');
	let folderText = $state('');
	let idea = $state('');
	let wanted = $state(6);
	let writing = $state(false);
	let playing = $state<number | null>(null);
	let player = $state<HTMLVideoElement | null>(null);

	const video = $derived(app.videoPackage);
	const lengths = $derived(videoLengthsOf(video));
	const shapes = $derived(videoSizesOf(video));
	const project = $derived(view?.project ?? null);
	const shape = $derived((Object.keys(shapes) as SizeName[]).find((n) => `${shapes[n].width}x${shapes[n].height}` === project?.size) ?? 'landscape');
	const done = $derived(project?.shots.filter((s) => s.state === 'done') ?? []);
	const todo = $derived(project?.shots.filter((s) => s.state !== 'done' && s.prompt.trim()).length ?? 0);
	const total = $derived(project ? project.shots.reduce((n, s) => n + s.frames / 24, 0) : 0);
	const clock = (s: number) => (s >= 60 ? `${Math.floor(s / 60)} min ${Math.round(s % 60)} s` : `${Math.round(s)} s`);
	const say = (e: unknown) => plain(e).text;

	const urls = new Map<string, string>();
	let clips = $state<Record<string, string>>({});
	async function clipOf(shot: StudioShot) {
		if (!project || shot.state !== 'done') return;
		const key = `${shot.id}:${shot.seconds ?? 0}`;
		if (urls.has(key)) return;
		try {
			const url = URL.createObjectURL(await api.studioClip(project.id, shot.id));
			urls.set(key, url);
			clips = { ...clips, [shot.id]: url };
		} catch {
			// The file was moved out of the folder: the row says so by having no picture.
		}
	}
	$effect(() => {
		for (const s of done) clipOf(s);
	});

	async function refresh() {
		try {
			films = await api.studioList();
		} catch (e) {
			problem = say(e);
		}
	}

	async function openFolder(folder: string) {
		problem = '';
		try {
			view = await api.studioOpen(folder);
			app.track('studio_opened', { shots: view.project.shots.length });
			refresh();
		} catch (e) {
			problem = say(e);
		}
	}

	async function newFilm() {
		const folder = (await api.pickFolder('Choose a folder for the film')) ?? folderText.trim();
		if (folder) await openFolder(folder);
	}

	async function openFilm(id: string) {
		problem = '';
		try {
			view = await api.studioRead(id);
		} catch (e) {
			problem = say(e);
		}
	}

	// Edits are kept on screen and saved a moment after the last change.
	let timer: ReturnType<typeof setTimeout> | undefined;
	function saveSoon() {
		clearTimeout(timer);
		timer = setTimeout(saveNow, 500);
	}
	async function saveNow() {
		clearTimeout(timer);
		timer = undefined;
		if (!project) return;
		try {
			const saved = await api.studioSave(project.id, { name: project.name, size: project.size, shots: project.shots.map(({ id, prompt, frames }) => ({ id, prompt, frames })) });
			// Keep what is being typed; take the states.
			if (view && !timer) view = saved;
		} catch (e) {
			problem = say(e);
		}
	}

	function addShot(prompt = '') {
		if (!project) return;
		project.shots.push({ id: '', prompt, frames: video?.defaults?.frames ?? lengths[0].frames, state: 'empty' });
		saveSoon();
	}

	function removeShot(i: number) {
		project?.shots.splice(i, 1);
		saveSoon();
	}

	function move(i: number, by: number) {
		if (!project) return;
		const to = i + by;
		if (to < 0 || to >= project.shots.length) return;
		const [shot] = project.shots.splice(i, 1);
		project.shots.splice(to, 0, shot);
		saveSoon();
	}

	function setShape(name: SizeName) {
		if (!project) return;
		project.size = `${shapes[name].width}x${shapes[name].height}`;
		saveSoon();
	}

	async function write() {
		if (!project || !idea.trim() || writing) return;
		writing = true;
		problem = '';
		try {
			const shots = await api.studioStoryboard(project.id, idea.trim(), wanted);
			for (const s of shots) project.shots.push({ id: '', prompt: s.prompt, frames: video?.defaults?.frames ?? lengths[0].frames, state: 'empty' });
			await saveNow();
			app.track('studio_shots_written', { shots: shots.length });
		} catch (e) {
			problem = say(e);
		} finally {
			writing = false;
		}
	}

	async function make(shots?: string[]) {
		if (!project) return;
		problem = '';
		try {
			await saveNow();
			view = await api.studioRender(project.id, shots);
			app.track('studio_make_clicked', { shots: shots?.length ?? todo });
		} catch (e) {
			problem = say(e);
		}
	}

	async function stop() {
		if (!project) return;
		try {
			view = await api.studioStop(project.id);
		} catch (e) {
			problem = say(e);
		}
	}

	// While shots are being made, look again every two seconds (not while something is being typed).
	const poll = setInterval(async () => {
		if (!view?.rendering || timer) return;
		try {
			const fresh = await api.studioRead(view.project.id);
			if (!timer && view && fresh.project.id === view.project.id) view = fresh;
		} catch {
			// The service is restarting; the next look will say.
		}
	}, 2000);

	// The film, played shot after shot.
	function play(from = 0) {
		if (done.length === 0) return;
		playing = from;
	}
	function next() {
		if (playing === null) return;
		playing = playing + 1 < done.length ? playing + 1 : null;
	}
	$effect(() => {
		if (playing !== null && player) player.play().catch(() => {});
	});

	onMount(refresh);
	onDestroy(() => {
		clearInterval(poll);
		clearTimeout(timer);
		for (const url of urls.values()) URL.revokeObjectURL(url);
	});

	const stateWords: Record<StudioShot['state'], string> = { empty: 'Not made', queued: 'Waiting', making: 'Being made', done: 'Made', failed: 'Did not work', changed: 'Changed' };
</script>

<section class="studio scroll" aria-label="Studio">
	<div class="column">
		{#if !project}
			<header class="hello">
				<h1>Make a film, shot by shot</h1>
				<p class="lede">A film is a list of shots in a folder of yours. Describe each shot, or let the chat model write them from an idea. The video model makes them one after another, with sound.</p>
				<p class="row">
					<button class="btn primary large" onclick={newFilm}>New film…</button>
				</p>
				{#if !api.isTauri}
					<p class="row"><input class="field" bind:value={folderText} placeholder="/full/path/to/a/folder" aria-label="Folder for the film" /></p>
				{/if}
				{#if !video}<p class="note">No video model is installed yet. You can plan a film now and add Video under <button class="link" onclick={() => app.go('models')}>Models</button> before making it.</p>{/if}
			</header>
			{#if films.length > 0}
				<h2 class="sub">Your films</h2>
				<ul class="films">
					{#each films as f (f.id)}
						<li>
							<button class="film" onclick={() => openFilm(f.id)} disabled={f.missing}>
								<Icon name="video" size={16} />
								<span class="fname">{f.name || 'Film'}</span>
								<span class="fmeta">{f.missing ? 'The folder is gone' : `${f.made ?? 0} of ${f.shots ?? 0} shots made`}</span>
								<span class="fpath selectable">{f.folder.replace(/^\/Users\/[^/]+/, '~')}</span>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
		{:else}
			<header class="top">
				<button class="btn quiet" onclick={() => ((view = null), refresh())}>← Films</button>
				<input class="name" bind:value={project.name} oninput={saveSoon} aria-label="Name of the film" />
				<span class="grow"></span>
				{#if view?.rendering}
					<button class="btn" onclick={stop}>Stop</button>
				{:else}
					<button class="btn" onclick={() => play(0)} disabled={done.length === 0}>Play film</button>
					<button class="btn primary" onclick={() => make()} disabled={todo === 0 || !video} title={video ? '' : 'Add Video under Models first'}>Make {todo > 0 ? `${todo} ${todo === 1 ? 'shot' : 'shots'}` : 'film'}</button>
				{/if}
			</header>
			<p class="facts">
				<span>{project.shots.length} {project.shots.length === 1 ? 'shot' : 'shots'}, {clock(total)}</span>
				<span>{done.length} made</span>
				<span class="chips" role="radiogroup" aria-label="Shape of the film">
					{#each Object.entries(shapes) as [name, s] (name)}
						<button type="button" role="radio" aria-checked={shape === name} onclick={() => setShape(name as SizeName)} disabled={view?.rendering} title={s.label} aria-label={s.label}><span class="shape" style:aspect-ratio="{s.width} / {s.height}"></span></button>
					{/each}
				</span>
				<span class="path selectable" title={project.folder}>{project.folder.replace(/^\/Users\/[^/]+/, '~')}</span>
			</p>
			{#if view?.progress}
				{@const p = view.progress}
				<p class="progress" role="status">
					<span class="dot"></span>
					Making shot {project.shots.findIndex((s) => s.id === p.shot) + 1} of {project.shots.length}{p.step !== null && p.steps ? `, step ${p.step} of ${p.steps}` : ''}, {clock(p.elapsed_ms / 1000)} so far. It keeps going if you leave this screen.
				</p>
			{/if}
			{#if problem}<p class="problem selectable" role="alert">{problem}</p>{/if}

			{#if playing !== null && done[playing]}
				<div class="stage">
					<!-- svelte-ignore a11y_media_has_caption -->
					<video bind:this={player} src={clips[done[playing].id]} controls autoplay onended={next}></video>
					<p class="caption">Shot {project.shots.indexOf(done[playing]) + 1}: {done[playing].prompt}</p>
					<button class="btn quiet" onclick={() => (playing = null)}>Close</button>
				</div>
			{/if}

			<div class="idea">
				<textarea bind:value={idea} rows="2" placeholder="What is the film about? For example: a red fox finds a paper boat in a snowy forest and follows it down a stream" aria-label="What the film is about"></textarea>
				<p class="row">
					<label>Shots <select bind:value={wanted}>{#each [3, 4, 6, 8, 12, 16, 24] as n (n)}<option value={n}>{n}</option>{/each}</select></label>
					<button class="btn" onclick={write} disabled={!idea.trim() || writing || app.chatPackages.length === 0}>{writing ? 'Writing the shots…' : 'Write the shots'}</button>
					{#if app.chatPackages.length === 0}<span class="note">Needs a chat model.</span>{/if}
				</p>
			</div>

			<ol class="shots">
				{#each project.shots as shot, i (shot.id || i)}
					<li class="shot" class:making={shot.state === 'making'}>
						<span class="n">{i + 1}</span>
						<div class="what">
							<textarea bind:value={shot.prompt} oninput={saveSoon} rows="3" placeholder="What is seen, what moves, the place, the light, the sound" aria-label="Shot {i + 1}"></textarea>
							<p class="row">
								<select bind:value={shot.frames} onchange={saveSoon} aria-label="Length of shot {i + 1}">
									{#each lengths as l (l.frames)}<option value={l.frames}>{l.label}</option>{/each}
								</select>
								<span class="state {shot.state}">{stateWords[shot.state]}{shot.state === 'done' && shot.seconds ? `, took ${clock(shot.seconds)}` : ''}</span>
								{#if shot.error}<span class="problem selectable">{shot.error}</span>{/if}
								<span class="grow"></span>
								<button class="tool" onclick={() => move(i, -1)} disabled={i === 0} title="Move up" aria-label="Move shot {i + 1} up">↑</button>
								<button class="tool" onclick={() => move(i, 1)} disabled={i === project.shots.length - 1} title="Move down" aria-label="Move shot {i + 1} down">↓</button>
								{#if shot.id && shot.prompt.trim() && video}<button class="tool" onclick={() => make([shot.id])} disabled={shot.state === 'making' || shot.state === 'queued'} title={shot.state === 'done' ? 'Make this shot again' : 'Make this shot'}>{shot.state === 'done' ? 'Again' : 'Make'}</button>{/if}
								<button class="tool" onclick={() => removeShot(i)} title="Remove this shot" aria-label="Remove shot {i + 1}"><Icon name="trash" size={13} /></button>
							</p>
						</div>
						<div class="clip">
							{#if clips[shot.id]}
								<!-- svelte-ignore a11y_media_has_caption -->
								<video src={clips[shot.id]} controls preload="metadata" class:stale={shot.state === 'changed'}></video>
							{:else}
								<span class="blank" aria-hidden="true"><Icon name="video" size={18} /></span>
							{/if}
						</div>
					</li>
				{/each}
			</ol>
			<p class="row end">
				<button class="btn" onclick={() => addShot()}>Add a shot</button>
				{#if video?.credit}<span class="note">{video.credit}</span>{/if}
			</p>
		{/if}
		{#if !project && problem}<p class="problem selectable" role="alert">{problem}</p>{/if}
	</div>
</section>

<style>
	.studio {
		height: 100%;
	}

	.column {
		width: 100%;
		max-width: 860px;
		margin: 0 auto;
		padding: 8px var(--gutter) 48px;
		display: grid;
		gap: 16px;
		align-content: start;
	}

	.hello {
		display: grid;
		justify-items: center;
		gap: 14px;
		padding: 12vh 0 8px;
		text-align: center;
	}

	h1 {
		font: 600 clamp(24px, 3vw, 32px) / 1.2 var(--rb-text);
		letter-spacing: -0.02em;
	}

	.lede {
		max-width: 52ch;
		color: var(--muted);
		font-size: 15px;
	}

	.note {
		color: var(--faint);
		font-size: 13px;
	}

	.sub {
		font: 600 13px/1 var(--rb-text);
		color: var(--faint);
	}

	.row {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
	}

	.end {
		justify-content: space-between;
	}

	.grow {
		flex: 1;
	}

	.field,
	.name,
	textarea,
	select {
		border: 1px solid var(--line);
		border-radius: 10px;
		background: var(--bg);
		color: var(--ink);
		font: 400 14px/1.45 var(--rb-text);
	}

	.field {
		width: min(520px, 100%);
		height: 36px;
		padding: 0 12px;
	}

	select {
		height: 28px;
		padding: 0 6px;
		border-radius: 8px;
		font-size: 13px;
	}

	textarea {
		width: 100%;
		padding: 10px 12px;
		resize: vertical;
	}

	textarea:focus,
	.name:focus,
	.field:focus {
		outline: none;
		border-color: var(--accent);
	}

	.films {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 4px;
	}

	.film {
		width: 100%;
		display: grid;
		grid-template-columns: auto minmax(0, 1fr) auto;
		align-items: center;
		gap: 2px 10px;
		padding: 10px 12px;
		border-radius: 10px;
		text-align: left;
	}

	.film:hover:not(:disabled) {
		background: var(--hover);
	}

	.fname {
		font-weight: 600;
	}

	.fmeta,
	.fpath {
		font-size: 12.5px;
		color: var(--faint);
	}

	.fpath {
		grid-column: 2 / -1;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.top {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.name {
		flex: 0 1 320px;
		min-width: 0;
		height: 34px;
		padding: 0 10px;
		border-color: transparent;
		font: 600 18px/1 var(--rb-text);
	}

	.name:hover {
		border-color: var(--line);
	}

	.facts {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 6px 16px;
		font-size: 13px;
		color: var(--muted);
	}

	.path {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--faint);
	}

	.chips {
		display: inline-flex;
		gap: 2px;
	}

	.chips button {
		width: 30px;
		height: 26px;
		display: grid;
		place-items: center;
		border-radius: 8px;
		color: var(--muted);
	}

	.chips button[aria-checked='true'] {
		background: var(--pressed);
		color: var(--ink);
	}

	.shape {
		height: 11px;
		max-width: 16px;
		border: 1.5px solid currentColor;
		border-radius: 2px;
	}

	.progress {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 10px 12px;
		border-radius: 10px;
		background: var(--accent-soft);
		font-size: 13.5px;
	}

	.dot {
		flex: none;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}

	.problem {
		color: var(--fail);
		font-size: 13px;
	}

	.stage {
		display: grid;
		gap: 8px;
		justify-items: start;
		padding: 12px;
		border-radius: 14px;
		background: var(--well);
	}

	.stage video {
		width: 100%;
		border-radius: 8px;
		background: #000;
	}

	.caption {
		font-size: 13px;
		color: var(--muted);
	}

	.idea {
		display: grid;
		gap: 8px;
		padding: 12px;
		border: 1px solid var(--line-soft);
		border-radius: 14px;
	}

	.idea label {
		display: inline-flex;
		align-items: center;
		gap: 6px;
		font-size: 13px;
		color: var(--muted);
	}

	.shots {
		margin: 0;
		padding: 0;
		list-style: none;
		display: grid;
		gap: 10px;
	}

	.shot {
		display: grid;
		grid-template-columns: 22px minmax(0, 1fr) 200px;
		gap: 10px;
		align-items: start;
		padding: 10px;
		border: 1px solid var(--line-soft);
		border-radius: 14px;
	}

	.shot.making {
		border-color: var(--accent);
	}

	.n {
		padding-top: 10px;
		text-align: right;
		font: 600 13px/1 var(--rb-text);
		color: var(--faint);
	}

	.what {
		display: grid;
		gap: 6px;
	}

	.state {
		font-size: 12.5px;
		color: var(--faint);
	}

	.state.done {
		color: var(--fit);
	}

	.state.failed {
		color: var(--fail);
	}

	.state.making,
	.state.queued {
		color: var(--accent);
	}

	.tool {
		min-width: 26px;
		height: 26px;
		padding: 0 7px;
		display: inline-grid;
		place-items: center;
		border-radius: 8px;
		font-size: 12.5px;
		color: var(--muted);
	}

	.tool:hover:not(:disabled) {
		background: var(--hover);
		color: var(--ink);
	}

	.tool:disabled {
		opacity: 0.35;
	}

	.clip video,
	.blank {
		width: 100%;
		aspect-ratio: 5 / 3;
		border-radius: 8px;
		background: var(--well);
	}

	.clip video {
		display: block;
		object-fit: cover;
		background: #000;
	}

	.clip video.stale {
		opacity: 0.45;
	}

	.blank {
		display: grid;
		place-items: center;
		color: var(--faint);
	}

	@media (max-width: 820px) {
		.shot {
			grid-template-columns: 22px minmax(0, 1fr);
		}
		.clip {
			grid-column: 2;
		}
	}
</style>
