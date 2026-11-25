<script lang="ts">
	import * as api from '#lib/api.ts';
	import { videoLengthsOf, videoSizesOf, type SizeName } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { remembered } from '#lib/remember.ts';
	import { app } from '#lib/state.svelte.ts';
	import { fileName } from '#lib/studio-look.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { filmWork, roughly } from '#lib/studio.ts';
	import StudioIcon from './StudioIcon.svelte';
	import StudioNeeds from './StudioNeeds.svelte';

	// Before a film is open: three ways to start one. Simple is one box and one button, and the
	// service directs the film from the words. Detailed sets everything the director would choose.
	// From a video cuts a file into shots. Under them, the films this install knows.

	type Way = 'simple' | 'detailed' | 'video';
	const ways: [Way, string][] = [
		['simple', 'Simple'],
		['detailed', 'Detailed'],
		['video', 'From a video']
	];
	// Pro starts with everything on the table; a choice made here is kept.
	const kept = remembered.get('studio.start');
	let way = $state<Way>(kept === 'simple' || kept === 'detailed' || kept === 'video' ? kept : ux.level === 'pro' ? 'detailed' : 'simple');
	const pick = (next: Way) => ((way = next), remembered.set('studio.start', next));

	const video = $derived(app.videoPackage);
	const shapes = $derived(videoSizesOf(video));
	const lengths = $derived(videoLengthsOf(video));
	/** The shot length closest to five seconds. */
	const five = $derived(lengths.reduce((best, l) => (Math.abs(l.frames - 121) < Math.abs(best.frames - 121) ? l : best), lengths[0]).frames);
	const ready = $derived(Boolean(video) && app.chatPackages.length > 0);
	const tilde = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');

	let about = $state('');
	let stage = $state<'' | 'directing' | 'writing' | 'starting' | 'bringing'>('');

	// What Detailed sets. "auto" leaves a choice to the director.
	const filmLengths = [15, 30, 60, 120, 300] as const;
	const lengthLabel = (s: number) => (s < 60 ? `${s} s` : `${s / 60} min`);
	let shape = $state<SizeName | 'auto'>('auto');
	let seconds = $state<number | 'auto'>('auto');
	let shotCount = $state('');
	let look = $state('');
	let cast = $state<{ name: string; look: string }[]>([]);
	let speech = $state<'auto' | 'off' | 'on'>('auto');
	let language = $state('');
	let voice = $state('');
	let subtitles = $state(true);
	let asWritten = $state(false);
	let folder = $state('');
	let askFolder = $state(false);

	const written = $derived(about.split('\n').filter((l) => l.trim()).length);
	const planned = $derived(asWritten ? Math.min(60, Math.max(1, written)) : Number(shotCount) > 0 ? Math.min(60, Math.floor(Number(shotCount))) : seconds === 'auto' ? 0 : Math.max(1, Math.round(seconds / (five / 24))));
	const work = $derived(planned > 0 && studio.pace ? filmWork(planned, five, studio.pace, studio.carries) : 0);

	async function changeFolder() {
		const chosen = await api.pickFolder('Choose a folder for the film');
		if (chosen) folder = chosen;
		else if (!api.isTauri) askFolder = true;
	}

	const said = (at: 'directing' | 'writing' | 'starting') => (stage = at);

	async function make() {
		const text = about.trim();
		if (!text || stage || !ready) return;
		let ok = false;
		try {
			if (way === 'simple') ok = await studio.direct(text, {}, said);
			else {
				const members = cast.map((m) => ({ name: m.name.trim(), look: m.look.trim() })).filter((m) => m.name);
				// Shots used as written have no director: what is not set here is the usual.
				const chosen = shape === 'auto' ? (asWritten ? 'landscape' : undefined) : shape;
				ok = await studio.direct(
					text,
					{
						shape: chosen,
						size: chosen ? `${shapes[chosen].width}x${shapes[chosen].height}` : undefined,
						seconds: seconds === 'auto' ? undefined : seconds,
						shots: planned > 0 ? planned : undefined,
						frames: five,
						look: look.trim() || undefined,
						cast: members.length > 0 ? members : undefined,
						speech: speech === 'auto' ? (asWritten ? false : undefined) : speech === 'on',
						language: language.trim() || undefined,
						voice: voice === '' ? null : Number(voice),
						subtitles,
						asWritten,
						folder
					},
					said
				);
			}
		} finally {
			stage = '';
		}
		if (ok) about = '';
	}

	// --- from a video --------------------------------------------------------------------------

	let file = $state('');

	async function chooseVideo() {
		const chosen = await api.pickFile('video', 'Choose a video');
		if (chosen) file = chosen;
	}

	async function bring() {
		if (!file.trim() || stage) return;
		stage = 'bringing';
		try {
			if (await studio.fromVideo(file.trim(), folder)) file = '';
		} finally {
			stage = '';
		}
	}

	async function empty() {
		const chosen = (await api.pickFolder('Choose a folder for the film')) ?? folder.trim();
		if (chosen) await studio.openFolder(chosen);
		else askFolder = true;
	}

	const doing = $derived(stage === 'directing' ? 'Directing…' : stage === 'writing' ? 'Writing the shots…' : stage === 'starting' ? 'Starting…' : stage === 'bringing' ? 'Bringing it in…' : '');
</script>

{#snippet folderRow()}
	<div class="st-row">
		<span class="st-lab">Folder</span>
		<span class="path selectable" title={folder.trim() || studio.home}>{folder.trim() ? tilde(folder.trim()) : `${studio.home ? tilde(studio.home) : 'Movies/Rigbrew'}, a new folder`}</span>
		{#if folder.trim()}<button class="st-btn quiet" onclick={() => ((folder = ''), (askFolder = false))} disabled={stage !== ''}>Usual</button>{/if}
		<button class="st-btn" onclick={changeFolder} disabled={stage !== ''}>Change</button>
	</div>
	{#if askFolder}<input class="st-in wide" bind:value={folder} placeholder="/full/path/to/a/folder" aria-label="Folder for the film" />{/if}
{/snippet}

<div class="start st-scroll">
	<div class="column">
		<header class="hello">
			<span class="mark"><StudioIcon name="film" size={16} /></span>
			<h1>New film</h1>
			<span class="st-seg ways" role="radiogroup" aria-label="How to start">
				{#each ways as [id, label] (id)}
					<button type="button" role="radio" aria-checked={way === id} onclick={() => pick(id)} disabled={stage !== ''}>{label}</button>
				{/each}
			</span>
		</header>

		{#if way !== 'video' && !ready}
			<StudioNeeds />
		{:else if way === 'simple'}
			<div class="box">
				<textarea
					class="st-ta idea"
					bind:value={about}
					rows="4"
					placeholder="What is the film about?"
					aria-label="What is the film about?"
					disabled={stage !== ''}
					onkeydown={(e) => (e.metaKey || e.ctrlKey) && e.key === 'Enter' && make()}
				></textarea>
				<div class="st-row">
					<button class="st-btn primary large" onclick={make} disabled={!about.trim() || stage !== ''}>{doing || 'Make the film'}</button>
					{#if doing}<span class="st-note" role="status">{stage === 'directing' ? 'Shape, length, look and cast are being chosen.' : ''}</span>{/if}
				</div>
			</div>
		{:else if way === 'detailed'}
			<div class="box">
				<textarea
					class="st-ta idea"
					bind:value={about}
					rows={asWritten ? 6 : 4}
					placeholder={asWritten ? 'One shot on each line' : 'What is the film about?'}
					aria-label={asWritten ? 'The shots, one on each line' : 'What is the film about?'}
					disabled={stage !== ''}
				></textarea>
				<div class="grid">
					<div class="st-row">
						<span class="st-lab">Shots</span>
						<span class="st-seg" role="radiogroup" aria-label="How the shots are written">
							<button type="button" role="radio" aria-checked={!asWritten} onclick={() => (asWritten = false)} title="The chat model writes each shot for the video model: subject, action, camera, light, sound">Enhanced</button>
							<button type="button" role="radio" aria-checked={asWritten} onclick={() => (asWritten = true)} title="Every line of the text is one shot, as it stands">As written</button>
						</span>
						{#if !asWritten}
							<span class="st-lab right">Number</span>
							<input class="st-in tiny" type="number" min="1" max="60" bind:value={shotCount} placeholder={planned > 0 ? String(planned) : 'auto'} aria-label="Number of shots" />
						{:else}
							<span class="st-val left">{written} {written === 1 ? 'shot' : 'shots'}</span>
						{/if}
					</div>
					<div class="st-row">
						<span class="st-lab">Shape</span>
						<span class="st-seg" role="radiogroup" aria-label="Shape of the film">
							<button type="button" role="radio" aria-checked={shape === 'auto'} onclick={() => (shape = 'auto')}>Auto</button>
							{#each Object.entries(shapes) as [name, s] (name)}
								<button type="button" role="radio" aria-checked={shape === name} onclick={() => (shape = name as SizeName)} title="{s.label}, {s.width} × {s.height}" aria-label={s.label}><span class="shape" style:aspect-ratio="{s.width} / {s.height}"></span></button>
							{/each}
						</span>
					</div>
					{#if !asWritten}
						<div class="st-row">
							<span class="st-lab">Length</span>
							<span class="st-seg" role="radiogroup" aria-label="Length of the film">
								<button type="button" role="radio" aria-checked={seconds === 'auto'} onclick={() => (seconds = 'auto')}>Auto</button>
								{#each filmLengths as l (l)}
									<button type="button" role="radio" aria-checked={seconds === l} onclick={() => (seconds = l)}>{lengthLabel(l)}</button>
								{/each}
							</span>
						</div>
					{/if}
					<div class="st-row">
						<span class="st-lab">Look</span>
						<input class="st-in grow" bind:value={look} maxlength="300" placeholder={asWritten ? 'Style, colours, light, lens' : 'Auto'} aria-label="The look every shot shares" />
					</div>
					<div class="st-row top">
						<span class="st-lab">Cast</span>
						<div class="cast">
							{#each cast as m, i (i)}
								<div class="st-row">
									<input class="st-in name" bind:value={m.name} maxlength="40" placeholder="Name" aria-label="Name of character {i + 1}" />
									<input class="st-in grow" bind:value={m.look} maxlength="400" placeholder="Age, build, face, hair, clothes, colours" aria-label="Look of character {i + 1}" />
									<button class="st-btn quiet icon" onclick={() => cast.splice(i, 1)} aria-label="Remove character {i + 1}" title="Remove"><StudioIcon name="close" size={10} /></button>
								</div>
							{/each}
							<div class="st-row">
								<button class="st-btn" onclick={() => cast.push({ name: '', look: '' })} disabled={cast.length >= 12}><StudioIcon name="plus" size={11} /> Character</button>
								{#if cast.length === 0 && !asWritten}<span class="st-note">Auto</span>{/if}
							</div>
						</div>
					</div>
					{#if !asWritten}
						<div class="st-row">
							<span class="st-lab">Speech</span>
							<span class="st-seg" role="radiogroup" aria-label="Whether the film has spoken lines">
								<button type="button" role="radio" aria-checked={speech === 'auto'} onclick={() => (speech = 'auto')}>Auto</button>
								<button type="button" role="radio" aria-checked={speech === 'off'} onclick={() => (speech = 'off')}>None</button>
								<button type="button" role="radio" aria-checked={speech === 'on'} onclick={() => (speech = 'on')}>Lines</button>
							</span>
							<span class="st-lab right">Language</span>
							<input class="st-in lang" bind:value={language} maxlength="24" placeholder="Auto" aria-label="Language of the lines" disabled={speech === 'off'} />
						</div>
					{/if}
					<div class="st-row">
						<span class="st-lab">Voice</span>
						<select class="st-in" bind:value={voice} aria-label="Voice of the narrator" disabled={!app.voicePackage}>
							<option value="">{app.voicePackage ? 'Usual' : 'No voice installed'}</option>
							{#each { length: studio.voices } as _, n (n)}<option value={String(n)}>Voice {n + 1}</option>{/each}
						</select>
						<label class="st-check right"><input type="checkbox" bind:checked={subtitles} /> Subtitles</label>
					</div>
					{@render folderRow()}
				</div>
				<div class="st-row">
					<button class="st-btn primary large" onclick={make} disabled={!about.trim() || stage !== ''}>{doing || 'Make the film'}</button>
					{#if !doing && planned > 0}
						<span class="st-note">{planned} {planned === 1 ? 'shot' : 'shots'}{work > 0 ? `, ${roughly(work)}${studio.pace?.measured ? '' : ' as a first guess'}` : ''}</span>
					{/if}
				</div>
			</div>
		{:else}
			<div class="box">
				<div class="st-row">
					<span class="st-lab">Video</span>
					{#if api.isTauri}
						<span class="path selectable" title={file}>{file ? fileName(file) : 'None chosen'}</span>
						<button class="st-btn" onclick={chooseVideo} disabled={stage !== ''}>Choose…</button>
					{:else}
						<input class="st-in grow" bind:value={file} placeholder="/full/path/to/a/video.mp4" aria-label="Full path of a video file" onkeydown={(e) => e.key === 'Enter' && bring()} />
					{/if}
				</div>
				{@render folderRow()}
				<div class="st-row">
					<button class="st-btn primary large" onclick={bring} disabled={!file.trim() || stage !== ''}>{doing || 'Bring it in'}</button>
					<span class="st-note">Cut into shots where its scenes change.</span>
				</div>
			</div>
		{/if}

		{#if studio.problem}<p class="st-problem selectable" role="alert">{studio.problem}</p>{/if}

		<div class="st-row films-h">
			<h2 class="st-h">Films</h2>
			<span class="grow"></span>
			<button class="st-btn quiet" onclick={empty} title="A film without shots, or a film folder to open"><StudioIcon name="plus" size={11} /> Empty film</button>
		</div>
		{#if askFolder && way === 'simple'}
			<input class="st-in wide" bind:value={folder} placeholder="/full/path/to/a/folder" aria-label="Folder of an empty or existing film" onkeydown={(e) => e.key === 'Enter' && empty()} />
		{/if}
		{#if studio.films.length > 0}
			<ul class="films">
				{#each studio.films as f (f.id)}
					<li>
						<button class="film" onclick={() => studio.open(f.id)} disabled={f.missing}>
							<StudioIcon name="film" size={13} />
							<span class="fname">{f.name || 'Film'}</span>
							<span class="fpath selectable">{tilde(f.folder)}</span>
							<span class="fmeta">{f.missing ? 'folder gone' : `${f.made ?? 0}/${f.shots ?? 0}`}</span>
						</button>
					</li>
				{/each}
			</ul>
		{/if}
		{#if video?.credit}<p class="st-note credit">{video.credit}</p>{/if}
	</div>
</div>

<style>
	.start {
		flex: 1;
		min-height: 0;
		background: var(--st-bg);
		user-select: text;
		-webkit-user-select: text;
	}

	.column {
		width: 100%;
		max-width: 620px;
		margin: 0 auto;
		padding: 6vh 20px 40px;
		display: grid;
		gap: 10px;
	}

	.hello {
		display: flex;
		align-items: center;
		gap: 10px;
		min-height: 30px;
	}

	.mark {
		flex: none;
		width: 28px;
		height: 28px;
		display: grid;
		place-items: center;
		border-radius: 4px;
		background: linear-gradient(180deg, #3a3d45, #2b2d33);
		box-shadow:
			inset 0 1px 0 rgba(255, 255, 255, 0.1),
			0 0 0 1px #000;
		color: var(--st-ink);
	}

	h1 {
		flex: 1;
		font: 600 17px/1.15 var(--rb-display);
		letter-spacing: -0.01em;
		color: #fff;
	}

	.ways :global(button) {
		padding: 0 12px;
		height: 22px;
	}

	.box {
		display: grid;
		gap: 9px;
		padding: 10px;
		border-radius: 5px;
		background: var(--st-raised);
		box-shadow:
			inset 0 1px 0 rgba(255, 255, 255, 0.05),
			0 0 0 1px #000;
	}

	.box :global(.idea) {
		font-size: 13px;
		line-height: 1.45;
		min-height: 76px;
	}

	.grid {
		display: grid;
		gap: 7px;
		padding: 2px 0;
	}

	.grid :global(.st-lab),
	.box > :global(.st-row > .st-lab) {
		width: 62px;
	}

	.grid :global(.st-lab.right),
	.grid :global(.st-check.right) {
		width: auto;
		margin-left: auto;
	}

	.top {
		align-items: flex-start;
	}

	.top > :global(.st-lab) {
		padding-top: 4px;
	}

	.cast {
		flex: 1;
		min-width: 0;
		display: grid;
		gap: 5px;
	}

	.cast :global(.name) {
		width: 110px;
	}

	.grid :global(.lang) {
		width: 96px;
	}

	.shape {
		display: block;
		height: 10px;
		max-width: 16px;
		border: 1.5px solid currentColor;
		border-radius: 1.5px;
	}

	.path {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font: 500 10.5px/1.6 var(--rb-data);
		color: var(--st-muted);
	}

	.grow {
		flex: 1;
	}

	.films-h {
		margin-top: 14px;
	}

	.films {
		margin: 0;
		padding: 0;
		list-style: none;
		border-radius: 4px;
		background: var(--st-panel);
		box-shadow: 0 0 0 1px #000;
		overflow: hidden;
	}

	.film {
		width: 100%;
		height: 26px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 9px;
		text-align: left;
		color: var(--st-muted);
		border-bottom: 1px solid rgba(0, 0, 0, 0.45);
	}

	.film:hover:not(:disabled) {
		background: rgba(255, 255, 255, 0.05);
	}

	.fname {
		flex: none;
		max-width: 46%;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 12px;
		font-weight: 600;
		color: var(--st-ink);
	}

	.fpath {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 10.5px;
		color: var(--st-faint);
	}

	.fmeta {
		flex: none;
		font: 500 10.5px/1 var(--rb-data);
		color: var(--st-muted);
	}

	.credit {
		margin-top: 6px;
	}
</style>
