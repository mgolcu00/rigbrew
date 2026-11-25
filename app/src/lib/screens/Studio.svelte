<script lang="ts">
	import { onDestroy, onMount } from 'svelte';
	import * as api from '#lib/api.ts';
	import type { StudioListing, StudioShot, StudioView } from '#lib/api.ts';
	import { videoLengthsOf, videoSizesOf, type SizeName } from '#lib/chat.svelte.ts';
	import { plain } from '#lib/errors.ts';
	import { bytes as bytesOf } from '#lib/format.ts';
	import { app } from '#lib/state.svelte.ts';
	import { isMac } from '#lib/words.ts';
	import { filmPlan, filmWork, hasClip, remadeByMove, reorder, roughly, shotSeconds } from '#lib/studio.ts';
	import Icon from '#lib/components/Icon.svelte';
	import StudioPlayer from '#lib/components/StudioPlayer.svelte';
	import StudioTimeline from '#lib/components/StudioTimeline.svelte';

	// Studio: a film is a list of shots in a folder. Each shot is one description and one clip;
	// the video model makes them one after another, so a long film renders on its own. The chat
	// model can write the shots from an idea. Clips stay in the folder as plain files.
	// The start screen takes one sentence and a length and does all of it: folder, shots, clips, MP4.

	let films = $state<StudioListing[]>([]);
	let view = $state<StudioView | null>(null);
	let problem = $state('');
	let folderText = $state('');
	let idea = $state('');
	let wanted = $state(6);
	let writing = $state(false);

	const video = $derived(app.videoPackage);
	const lengths = $derived(videoLengthsOf(video));
	const shapes = $derived(videoSizesOf(video));
	const project = $derived(view?.project ?? null);
	const shape = $derived((Object.keys(shapes) as SizeName[]).find((n) => `${shapes[n].width}x${shapes[n].height}` === project?.size) ?? 'landscape');
	const done = $derived(project?.shots.filter((s) => s.state === 'done') ?? []);
	/** The film as it plays and as it is exported: the made shots with their trims. */
	const plan = $derived(filmPlan(project?.shots ?? []));
	const todo = $derived(project?.shots.filter((s) => s.state !== 'done' && s.prompt.trim()).length ?? 0);
	const total = $derived(project ? project.shots.reduce((n, s) => n + shotSeconds(s), 0) : 0);
	const clock = (s: number) => (s >= 60 ? `${Math.floor(s / 60)} min ${Math.round(s % 60)} s` : `${Math.round(s)} s`);
	const say = (e: unknown) => plain(e).text;
	const capital = (text: string) => text.charAt(0).toUpperCase() + text.slice(1);
	const waitingShots = $derived(project?.shots.filter((s) => s.state === 'making' || s.state === 'queued').length ?? 0);
	const tilde = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');

	// One sentence to a finished film.
	const filmLengths = [
		{ seconds: 15, label: 'About 15 s' },
		{ seconds: 30, label: '30 s' },
		{ seconds: 60, label: '1 min' },
		{ seconds: 120, label: '2 min' },
		{ seconds: 300, label: '5 min' }
	] as const;
	let about = $state('');
	let aboutSeconds = $state<number>(30);
	let aboutShape = $state<SizeName>('landscape');
	let aboutFolder = $state('');
	let askFolder = $state(false);
	let home = $state('');
	/** What a shot takes on this machine, and whether shots can continue each other, as the service last said. */
	let pace = $state<api.StudioPace | null>(null);
	let carries = $state(false);
	/** What the one-sentence film is doing: '' when idle. */
	let quick = $state<'' | 'folder' | 'writing' | 'starting'>('');
	/** The shot length closest to five seconds. */
	const five = $derived(lengths.reduce((best, l) => (Math.abs(l.frames - 121) < Math.abs(best.frames - 121) ? l : best), lengths[0]).frames);
	const quickShots = $derived(Math.max(1, Math.round(aboutSeconds / (five / 24))));
	const quickWork = $derived(pace ? filmWork(quickShots, five, pace, carries) : 0);

	/** A name for the film out of its first words. */
	function titleOf(text: string) {
		const words = text
			.replace(/[^\p{L}\p{N}\s'-]/gu, ' ')
			.split(/\s+/)
			.filter(Boolean)
			.slice(0, 6)
			.join(' ')
			.slice(0, 48)
			.trim();
		return words ? words[0].toUpperCase() + words.slice(1) : 'Film';
	}

	async function changeFolder() {
		const folder = await api.pickFolder('Choose a folder for the film');
		if (folder) aboutFolder = folder;
		else if (!api.isTauri) askFolder = true;
	}

	async function makeFilm() {
		const text = about.trim();
		if (!text || quick) return;
		problem = '';
		let opened: StudioView | null = null;
		try {
			quick = 'folder';
			opened = await api.studioOpen(aboutFolder.trim(), titleOf(text));
			const id = opened.project.id;
			if (opened.project.shots.length > 0) {
				// The folder already holds a film: it is opened as it is.
				view = opened;
				return;
			}
			quick = 'writing';
			const shots = await api.studioStoryboard(id, text, quickShots);
			quick = 'starting';
			const carries = Boolean(opened.can_continue);
			await api.studioSave(id, {
				size: `${shapes[aboutShape].width}x${shapes[aboutShape].height}`,
				auto_export: true,
				shots: shots.map((s, i) => ({ prompt: s.prompt, frames: five, continues: carries && i > 0 && Boolean(s.continues) }))
			});
			view = video ? await api.studioRender(id) : await api.studioRead(id);
			app.track('studio_quick_film', { seconds: aboutSeconds, shots: shots.length, shape: aboutShape, own_folder: Boolean(aboutFolder.trim()) });
			about = '';
		} catch (e) {
			// The folder is there: open the film so the idea can be tried again from inside it.
			if (opened) {
				view = opened;
				idea = text;
				wanted = quickShots;
			}
			problem = say(e);
		} finally {
			quick = '';
			refresh();
		}
	}

	// Clips are read from the service when they are played, and only the last few are kept: a long
	// film has more of them than fit in memory at once.
	const KEPT_CLIPS = 12;
	const clipUrls = new Map<string, Promise<string | null>>();
	function clipOf(id: string): Promise<string | null> {
		const shot = project?.shots.find((s) => s.id === id);
		if (!project || !shot) return Promise.resolve(null);
		const key = `${id}:${shot.take ?? 0}:${shot.seconds ?? 0}`;
		const film = project.id;
		let url = clipUrls.get(key);
		clipUrls.delete(key);
		url ??= api
			.studioClip(film, id)
			.then((blob) => URL.createObjectURL(blob))
			.catch(() => null);
		clipUrls.set(key, url);
		if (clipUrls.size > KEPT_CLIPS) {
			const [oldest, old] = clipUrls.entries().next().value!;
			clipUrls.delete(oldest);
			old.then((u) => u && URL.revokeObjectURL(u));
		}
		return url;
	}

	// One shot at a time plays in its row.
	let rowClip = $state<{ id: string; url: string } | null>(null);
	async function playRow(shot: StudioShot) {
		const url = await clipOf(shot.id);
		if (url) rowClip = { id: shot.id, url };
		else problem = 'The clip of this shot is not in the film\'s folder any more. Make the shot again.';
	}

	// One small picture per made shot, for the timeline.
	const pictureUrls = new Map<string, string>();
	let posters = $state<Record<string, string>>({});
	async function posterOf(shot: StudioShot) {
		if (!project || !shot.id || !hasClip(shot)) return;
		const key = `${shot.id}:${shot.take ?? 0}:${shot.seconds ?? 0}`;
		if (pictureUrls.has(key)) return;
		pictureUrls.set(key, '');
		try {
			const url = URL.createObjectURL(await api.studioPoster(project.id, shot.id));
			pictureUrls.set(key, url);
			posters = { ...posters, [shot.id]: url };
		} catch {
			pictureUrls.delete(key);
		}
	}
	// The takes of a shot with more than one, each with its own small picture.
	let takePosters = $state<Record<string, string>>({});
	async function takePosterOf(shot: StudioShot, n: number) {
		const key = `${shot.id}.${n}`;
		if (!project || pictureUrls.has(key)) return;
		pictureUrls.set(key, '');
		try {
			const url = URL.createObjectURL(await api.studioPoster(project.id, shot.id, n));
			pictureUrls.set(key, url);
			takePosters = { ...takePosters, [key]: url };
		} catch {
			pictureUrls.delete(key);
		}
	}
	$effect(() => {
		for (const s of project?.shots ?? []) {
			posterOf(s);
			if ((s.takes?.length ?? 0) > 1) for (const t of s.takes!) takePosterOf(s, t.n);
		}
	});

	const takeOf = (shot: StudioShot) => shot.takes?.find((t) => t.n === shot.take);

	async function useTake(shot: StudioShot, n: number) {
		if (!project || shot.take === n) return;
		problem = '';
		try {
			await saveNow();
			view = await api.studioUseTake(project.id, shot.id, n);
			if (rowClip?.id === shot.id) rowClip = null;
			app.track('studio_take_used', { takes: shot.takes?.length ?? 0 });
		} catch (e) {
			problem = say(e);
		}
	}

	/** The shot whose take in use is about to be deleted, asked about first. */
	let deleting = $state('');
	async function deleteTake(shot: StudioShot) {
		deleting = '';
		if (!project || !shot.take) return;
		problem = '';
		try {
			await saveNow();
			view = await api.studioDeleteTake(project.id, shot.id, shot.take);
			if (rowClip?.id === shot.id) rowClip = null;
		} catch (e) {
			problem = say(e);
		}
	}

	// The shot the timeline and the list both point at.
	let selected = $state(-1);
	function select(i: number, scroll = true) {
		selected = i;
		const id = project?.shots[i]?.id;
		if (scroll && id) document.getElementById(`shot-${id}`)?.scrollIntoView({ block: 'nearest', behavior: 'smooth' });
		if (scroll && id) player?.seekShot(id);
	}

	// The film, played as it will be exported.
	let showPlayer = $state(false);
	let player = $state<StudioPlayer | null>(null);
	let position = $state<{ shot: string; at: number } | null>(null);
	let soundUrl = $state<string | null>(null);
	let soundOf = '';
	async function openPlayer() {
		if (!project || plan.parts.length === 0) return;
		await saveNow();
		// The sound file is read again when another one was chosen.
		const wanted = project.sound?.trim() ?? '';
		if (wanted !== soundOf) {
			if (soundUrl) URL.revokeObjectURL(soundUrl);
			soundUrl = null;
			soundOf = wanted;
			if (wanted) {
				try {
					soundUrl = URL.createObjectURL(await api.studioSound(project.id));
				} catch {
					problem = 'The sound file was not found, or it is not a kind the app can play. The film plays without it.';
				}
			}
		}
		rowClip = null;
		showPlayer = true;
		app.track('studio_played', { shots: plan.parts.length, sound: Boolean(soundUrl) });
	}

	async function refresh() {
		try {
			const all = await api.studioList();
			films = all.projects;
			home = all.home ?? '';
			pace = all.pace ?? null;
			carries = Boolean(all.can_continue);
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
			const saved = await api.studioSave(project.id, {
				name: project.name,
				size: project.size,
				auto_export: Boolean(project.auto_export),
				sound: project.sound ?? '',
				sound_volume: project.sound_volume ?? 0.5,
				sound_once: Boolean(project.sound_once),
				fade_in: Boolean(project.fade_in),
				fade_out: Boolean(project.fade_out),
				export: project.export ?? {},
				shots: project.shots.map((s) => ({
					id: s.id,
					prompt: s.prompt,
					frames: s.frames,
					trim_start: s.trim_start ?? 0,
					trim_end: s.trim_end ?? 0,
					continues: Boolean(s.continues),
					join: s.join ?? 'cut',
					volume: s.volume ?? 1,
					mute: Boolean(s.mute),
					seed: s.seed ?? null
				}))
			});
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

	// A move that sends made shots back to be made again is asked about first.
	let pendingMove = $state<{ from: number; to: number; remade: number[] } | null>(null);
	function move(from: number, to: number) {
		if (!project || to < 0 || to >= project.shots.length || to === from) return;
		const remade = remadeByMove(project.shots, from, to);
		if (remade.length > 0) pendingMove = { from, to, remade };
		else moveNow(from, to);
	}
	function moveNow(from: number, to: number) {
		pendingMove = null;
		if (!project) return;
		project.shots = reorder(project.shots, from, to);
		selected = to;
		app.track('studio_shot_moved', { by: to - from });
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
			for (const s of shots) project.shots.push({ id: '', prompt: s.prompt, frames: video?.defaults?.frames ?? lengths[0].frames, state: 'empty', continues: Boolean(s.continues && view?.can_continue && project.shots.length > 0) });
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

	// How the film is written to its file: how large, how good, under which name.
	const [filmWidth, filmHeight] = $derived((project?.size ?? '640x384').split('x').map(Number));
	function setExport(change: api.StudioExportOptions) {
		if (!project) return;
		project.export = { ...project.export, ...change };
		saveSoon();
	}
	async function reveal() {
		if (!project) return;
		try {
			await api.studioReveal(project.id);
		} catch (e) {
			problem = say(e);
		}
	}

	// The voice reads a text, and the reading becomes the film's sound file.
	let narration = $state('');
	let narrating = $state(false);
	async function narrate() {
		if (!project || !narration.trim() || narrating) return;
		narrating = true;
		problem = '';
		try {
			await saveNow();
			view = await api.studioNarration(project.id, narration.trim());
			soundOf = '';
			app.track('studio_narration_made', { chars: narration.trim().length });
			narration = '';
		} catch (e) {
			problem = say(e);
		} finally {
			narrating = false;
		}
	}

	// One file out of the made shots, in the film's folder. The service says where the export is,
	// also for a film that exports by itself.
	const exported = $derived(view?.export ?? null);
	async function exportFilm() {
		if (!project) return;
		problem = '';
		try {
			await saveNow();
			await api.studioExport(project.id);
			app.track('studio_export_clicked', { shots: plan.parts.length, sound: Boolean(project.sound?.trim()), scale: project.export?.scale === 2 ? 2 : 1, quality: project.export?.quality || 'balanced' });
			view = await api.studioRead(project.id);
		} catch (e) {
			problem = say(e);
		}
	}

	// While shots are being made, look again every two seconds (not while something is being typed).
	const poll = setInterval(async () => {
		if (!(view?.rendering || view?.export?.state === 'working') || timer) return;
		try {
			const fresh = await api.studioRead(view.project.id);
			if (!timer && view && fresh.project.id === view.project.id) view = fresh;
		} catch {
			// The service is restarting; the next look will say.
		}
	}, 2000);

	onMount(refresh);
	onDestroy(() => {
		clearInterval(poll);
		clearTimeout(timer);
		for (const url of pictureUrls.values()) if (url) URL.revokeObjectURL(url);
		for (const url of clipUrls.values()) url.then((u) => u && URL.revokeObjectURL(u));
		if (soundUrl) URL.revokeObjectURL(soundUrl);
	});

	const stateWords: Record<StudioShot['state'], string> = { empty: 'Not made', queued: 'Waiting', making: 'Being made', done: 'Made', failed: 'Did not work', changed: 'Changed' };
</script>

<section class="studio scroll" aria-label="Studio">
	<div class="column">
		{#if !project}
			<header class="hello">
				<h1>Make a film</h1>
				<p class="lede">Say what the film is about. The chat model writes the shots, the video model makes them one after another with sound, and the film is saved as one MP4 file.</p>
			</header>
			<div class="quick">
				<textarea bind:value={about} rows="3" placeholder="What is the film about? For example: a red fox finds a paper boat in a snowy forest and follows it down a stream" aria-label="What is the film about?" disabled={quick !== ''}></textarea>
				<div class="row">
					<span class="label">Length</span>
					<span class="seg" role="radiogroup" aria-label="Length of the film">
						{#each filmLengths as l (l.seconds)}
							<button type="button" role="radio" aria-checked={aboutSeconds === l.seconds} onclick={() => (aboutSeconds = l.seconds)} disabled={quick !== ''}>{l.label}</button>
						{/each}
					</span>
					<span class="label">Shape</span>
					<span class="chips" role="radiogroup" aria-label="Shape of the film">
						{#each Object.entries(shapes) as [name, s] (name)}
							<button type="button" role="radio" aria-checked={aboutShape === name} onclick={() => (aboutShape = name as SizeName)} disabled={quick !== ''} title={s.label} aria-label={s.label}><span class="shape" style:aspect-ratio="{s.width} / {s.height}"></span></button>
						{/each}
					</span>
				</div>
				<p class="row">
					<button class="btn primary large" onclick={makeFilm} disabled={!about.trim() || quick !== '' || app.chatPackages.length === 0}>
						{quick === 'writing' ? 'Writing the shots…' : quick ? 'Starting…' : 'Make the film'}
					</button>
					{#if quick === 'writing'}
						<span class="note" role="status">The chat model is writing {quickShots} {quickShots === 1 ? 'shot' : 'shots'}. This can take a minute or two.</span>
					{:else}
						<span class="note">
							{quickShots} {quickShots === 1 ? 'shot' : 'shots'} of {Math.round(five / 24)} s.
							{#if video && quickWork > 0}This film will take {roughly(quickWork)} to make on this machine{pace?.measured ? '' : ', as a first guess'}.{/if}
							You can change every shot afterwards.
						</span>
					{/if}
				</p>
				<p class="note">
					{#if aboutFolder.trim()}
						It is saved in <span class="selectable">{tilde(aboutFolder.trim())}</span>.
						<button class="link" onclick={() => ((aboutFolder = ''), (askFolder = false))} disabled={quick !== ''}>Use the usual folder</button>
					{:else}
						It is saved in a new folder in <span class="selectable">{home ? tilde(home) : 'Movies/Rigbrew'}</span>.
						<button class="link" onclick={changeFolder} disabled={quick !== ''}>Change folder</button>
					{/if}
				</p>
				{#if askFolder}<input class="field" bind:value={aboutFolder} placeholder="/full/path/to/a/folder" aria-label="Folder for the film" />{/if}
				{#if app.chatPackages.length === 0}<p class="note">Writing the shots needs a chat model. Add one under <button class="link" onclick={() => app.go('models')}>Models</button>.</p>{/if}
				{#if !video}<p class="note">No video model is installed yet. The shots are written now and made once you add Video under <button class="link" onclick={() => app.go('models')}>Models</button>.</p>{/if}
			</div>
			{#if problem}<p class="problem selectable" role="alert">{problem}</p>{/if}
			<p class="row other">
				<button class="link" onclick={newFilm}>Start with an empty film</button>
				<span class="note">to write every shot yourself, or to open a film folder.</span>
			</p>
			{#if !api.isTauri}
				<p class="row"><input class="field" bind:value={folderText} placeholder="/full/path/to/a/folder" aria-label="Folder of an empty or existing film" /></p>
			{/if}
			{#if films.length > 0}
				<h2 class="sub">Your films</h2>
				<ul class="films">
					{#each films as f (f.id)}
						<li>
							<button class="film" onclick={() => openFilm(f.id)} disabled={f.missing}>
								<Icon name="video" size={16} />
								<span class="fname">{f.name || 'Film'}</span>
								<span class="fmeta">{f.missing ? 'The folder is gone' : `${f.made ?? 0} of ${f.shots ?? 0} shots made`}</span>
								<span class="fpath selectable">{tilde(f.folder)}</span>
							</button>
						</li>
					{/each}
				</ul>
			{/if}
			{#if video?.credit}<p class="note credit">{video.credit}</p>{/if}
		{:else}
			<header class="top">
				<button class="btn quiet" onclick={() => ((view = null), (showPlayer = false), (rowClip = null), (selected = -1), refresh())}>← Films</button>
				<input class="name" bind:value={project.name} oninput={saveSoon} aria-label="Name of the film" />
				<span class="grow"></span>
				{#if view?.rendering}
					<button class="btn" onclick={stop}>Stop</button>
				{:else}
					<button class="btn" onclick={openPlayer} disabled={plan.parts.length === 0} title="Plays the made shots one after another, as the exported film will be">Play film</button>
					<button class="btn" onclick={exportFilm} disabled={plan.parts.length === 0 || exported?.state === 'working'} title="Joins the made shots into one MP4 file in the film's folder">{exported?.state === 'working' ? 'Exporting…' : 'Export'}</button>
					<button class="btn primary" onclick={() => make()} disabled={todo === 0 || !video} title={video ? '' : 'Add Video under Models first'}>Make {todo > 0 ? `${todo} ${todo === 1 ? 'shot' : 'shots'}` : 'film'}</button>
				{/if}
			</header>
			<p class="facts">
				<span>{project.shots.length} {project.shots.length === 1 ? 'shot' : 'shots'}, {clock(total)}</span>
				<span>{done.length} made</span>
				{#if !view?.rendering && todo > 0 && video && view?.estimate}<span>Making the {todo === 1 ? 'shot that is left' : `${todo} shots that are left`} takes {roughly(view.estimate.all)}</span>{/if}
				<span class="chips" role="radiogroup" aria-label="Shape of the film">
					{#each Object.entries(shapes) as [name, s] (name)}
						<button type="button" role="radio" aria-checked={shape === name} onclick={() => setShape(name as SizeName)} disabled={view?.rendering} title={s.label} aria-label={s.label}><span class="shape" style:aspect-ratio="{s.width} / {s.height}"></span></button>
					{/each}
				</span>
				<span class="path selectable" title={project.folder}>{tilde(project.folder)}</span>
			</p>
			{#if view?.progress}
				{@const p = view.progress}
				<p class="progress" role="status">
					<span class="dot"></span>
					Making shot {project.shots.findIndex((s) => s.id === p.shot) + 1} of {project.shots.length}{p.step !== null && p.steps ? `, step ${p.step} of ${p.steps}` : ''}, {clock(p.elapsed_ms / 1000)} so far.
					{#if view.estimate && view.estimate.left > 0}{capital(roughly(view.estimate.left))} left{waitingShots > 1 ? ` for the ${waitingShots} shots` : ''}.{/if}
					It keeps going if you leave this screen.
				</p>
			{/if}
			{#if exported?.state === 'working'}
				<p class="progress" role="status"><span class="dot"></span>Joining {exported.shots} {exported.shots === 1 ? 'shot' : 'shots'} into one file.</p>
			{:else if exported?.state === 'done' && exported.file}
				<p class="progress" role="status">
					<span class="selectable saved">The film is saved{exported.seconds ? ` (${clock(exported.seconds)}${exported.bytes ? `, ${bytesOf(exported.bytes)}` : ''})` : ''}: {tilde(exported.file)}</span>
					<button class="btn" onclick={reveal}>{isMac ? 'Show in Finder' : 'Show in folder'}</button>
				</p>
			{:else if exported?.state === 'failed'}
				<p class="problem selectable" role="alert">The film could not be exported. {exported.error ?? ''}</p>
			{:else if project.auto_export && (view?.rendering || todo > 0)}
				<p class="note">The film is exported by itself when the last shot is made.</p>
			{/if}
			{#if problem}<p class="problem selectable" role="alert">{problem}</p>{/if}

			{#if showPlayer && plan.parts.length > 0}
				<StudioPlayer
					bind:this={player}
					parts={plan.parts}
					total={plan.total}
					shape={project.size}
					{clipOf}
					sound={soundUrl}
					soundVolume={project.sound_volume ?? 0.5}
					soundRepeats={!project.sound_once}
					fadeIn={Boolean(project.fade_in)}
					fadeOut={Boolean(project.fade_out)}
					caption={(part) => `Shot ${part.index + 1}: ${project.shots[part.index]?.prompt ?? ''}`}
					onposition={(at) => (position = at)}
					onclose={() => (showPlayer = false)}
				/>
			{/if}

			{#if project.shots.length > 0}
				<StudioTimeline shots={project.shots} {posters} {selected} playhead={showPlayer ? position : null} onselect={select} onmove={move} />
			{/if}
			{#if pendingMove}
				{@const n = pendingMove.remade.length}
				<p class="ask" role="alert">
					<span>
						{n === 1 ? `Shot ${pendingMove.remade[0] + 1} continues the shot before it.` : `${n} shots continue the shot before them.`}
						After this move {n === 1 ? 'it starts' : 'they start'} from a different picture, so {n === 1 ? 'it has' : 'they have'} to be made again.
					</span>
					<button class="btn" onclick={() => pendingMove && moveNow(pendingMove.from, pendingMove.to)}>Move it</button>
					<button class="btn quiet" onclick={() => (pendingMove = null)}>Leave it</button>
				</p>
			{/if}

			<div class="idea">
				<textarea bind:value={idea} rows="2" placeholder="What is the film about? For example: a red fox finds a paper boat in a snowy forest and follows it down a stream" aria-label="What the film is about"></textarea>
				<p class="row">
					<label>Shots <select bind:value={wanted}>{#each [3, 4, 6, 8, 12, 16, 24, 36, 60] as n (n)}<option value={n}>{n}</option>{/each}</select></label>
					<button class="btn" onclick={write} disabled={!idea.trim() || writing || app.chatPackages.length === 0}>{writing ? 'Writing the shots…' : 'Write the shots'}</button>
					{#if app.chatPackages.length === 0}<span class="note">Needs a chat model.</span>{/if}
				</p>
			</div>

			<ol class="shots" style:--shape="{project.size.replace('x', ' / ')}">
				{#each project.shots as shot, i (shot.id || i)}
					<li class="shot" class:making={shot.state === 'making'} class:on={i === selected} id="shot-{shot.id}" onfocusin={() => select(i, false)}>
						<span class="n">{i + 1}</span>
						<div class="what">
							<textarea bind:value={shot.prompt} oninput={saveSoon} rows="3" placeholder="What is seen, what moves, the place, the light, the sound" aria-label="Shot {i + 1}"></textarea>
							<p class="row">
								<select bind:value={shot.frames} onchange={saveSoon} aria-label="Length of shot {i + 1}" title="Make the shot shorter or longer. A made shot is then marked Changed until it is made again; the clip it has stays as a take.">
									{#each lengths as l (l.frames)}<option value={l.frames}>{l.label}</option>{/each}
								</select>
								<span class="state {shot.state}">{stateWords[shot.state]}{shot.state === 'done' && shot.seconds ? `, took ${clock(shot.seconds)}` : ''}</span>
								{#if shot.error}<span class="problem selectable">{shot.error}</span>{/if}
								{#if shot.state === 'done' || shot.state === 'changed'}
									<label class="trim" title="Seconds cut from the start and from the end of this shot in the exported film">Cut <input type="number" min="0" max="9" step="0.1" bind:value={shot.trim_start} oninput={saveSoon} aria-label="Seconds cut from the start of shot {i + 1}" /> <input type="number" min="0" max="9" step="0.1" bind:value={shot.trim_end} oninput={saveSoon} aria-label="Seconds cut from the end of shot {i + 1}" /> s</label>
								{/if}
								<span class="grow"></span>
								<button class="tool" onclick={() => move(i, i - 1)} disabled={i === 0} title="Move up" aria-label="Move shot {i + 1} up">↑</button>
								<button class="tool" onclick={() => move(i, i + 1)} disabled={i === project.shots.length - 1} title="Move down" aria-label="Move shot {i + 1} down">↓</button>
								{#if shot.id && shot.prompt.trim() && video}<button class="tool" onclick={() => make([shot.id])} disabled={shot.state === 'making' || shot.state === 'queued'} title={shot.state === 'done' ? 'Makes this shot again. The clip you have now is kept as a take.' : 'Make this shot'}>{shot.state === 'done' ? 'Try again' : 'Make'}</button>{/if}
								<button class="tool" onclick={() => removeShot(i)} title="Remove this shot" aria-label="Remove shot {i + 1}"><Icon name="trash" size={13} /></button>
							</p>
							<p class="row mix">
								{#if i > 0 && view?.can_continue}
									<label class="trim" title="This shot starts from the last picture of the shot before it, so the two join without a cut. It takes longer to make."><input type="checkbox" bind:checked={shot.continues} onchange={saveSoon} /> Continue the shot before</label>
								{/if}
								{#if i > 0 && !shot.continues}
									<span class="trim" title="How the film goes from the shot before into this one. A dissolve lays the two over each other for half a second. A fade through black takes the same half second.">
										Starts with
										<select value={shot.join ?? 'cut'} onchange={(e) => ((shot.join = e.currentTarget.value as StudioShot['join']), saveSoon())} aria-label="How the film goes into shot {i + 1}">
											<option value="cut">a cut</option>
											<option value="dissolve">a dissolve</option>
											<option value="black">a fade through black</option>
										</select>
									</span>
								{/if}
								<label class="trim" title="How loud this shot's own sound is in the film">
									Sound
									<input type="range" min="0" max="1.5" step="0.05" value={shot.volume ?? 1} oninput={(e) => ((shot.volume = Number(e.currentTarget.value)), saveSoon())} disabled={shot.mute} aria-label="Volume of shot {i + 1}" />
									<span class="pct">{shot.mute ? 'off' : `${Math.round((shot.volume ?? 1) * 100)}%`}</span>
								</label>
								<label class="trim"><input type="checkbox" bind:checked={shot.mute} onchange={saveSoon} /> Mute</label>
							</p>
						</div>
						<div class="clip">
							{#if rowClip?.id === shot.id}
								<!-- svelte-ignore a11y_media_has_caption -->
								<video src={rowClip.url} controls autoplay class:stale={shot.state === 'changed'}></video>
							{:else if hasClip(shot) && shot.id}
								<button class="poster" class:stale={shot.state === 'changed'} style:background-image={posters[shot.id] ? `url(${posters[shot.id]})` : undefined} onclick={() => playRow(shot)} aria-label="Play shot {i + 1}" title="Play this shot">
									<span class="go" aria-hidden="true"></span>
								</button>
							{:else}
								<span class="blank" aria-hidden="true"><Icon name="video" size={18} /></span>
							{/if}
							{#if hasClip(shot) && (shot.takes?.length ?? 0) > 0}
								{@const used = takeOf(shot)}
								{#if shot.takes!.length > 1}
									<div class="takes" role="group" aria-label="Takes of shot {i + 1}">
										{#each shot.takes! as t (t.n)}
											<button class="take" aria-pressed={t.n === shot.take} onclick={() => useTake(shot, t.n)} style:background-image={takePosters[`${shot.id}.${t.n}`] ? `url(${takePosters[`${shot.id}.${t.n}`]})` : undefined} title="Use take {t.n}" aria-label="Take {t.n}{t.n === shot.take ? ', in use' : ''}"></button>
										{/each}
									</div>
								{/if}
								{#if deleting === shot.id}
									<p class="takeline" role="alert">
										<span>Delete take {shot.take}? Its clip is removed from the folder.</span>
										<button class="tool danger" onclick={() => deleteTake(shot)}>Delete</button>
										<button class="tool" onclick={() => (deleting = '')}>Keep</button>
									</p>
								{:else if used}
									<p class="takeline">
										<span>Take {shot.takes!.findIndex((t) => t.n === shot.take) + 1} of {shot.takes!.length}</span>
										{#if used.seed !== undefined}
											<label title="With the same seed and the same description the video model makes the same clip again. Keep the seed and change the description a little to change the clip a little.">
												<input type="checkbox" checked={shot.seed === used.seed} onchange={(e) => ((shot.seed = e.currentTarget.checked ? used.seed : null), saveSoon())} /> Keep its seed
											</label>
										{/if}
										<button class="tool" onclick={() => (deleting = shot.id)} disabled={shot.state === 'making' || shot.state === 'queued'} title="Delete this take" aria-label="Delete take {shot.take} of shot {i + 1}"><Icon name="trash" size={12} /></button>
									</p>
								{/if}
							{/if}
						</div>
					</li>
				{/each}
			</ol>
			<p class="row end">
				<button class="btn" onclick={() => addShot()}>Add a shot</button>
			</p>

			<div class="panel">
				<h2 class="sub">Sound under the film</h2>
				<p class="row">
					<input class="field music" bind:value={project.sound} oninput={saveSoon} placeholder="Full path to a sound file: music or narration (optional)" aria-label="Sound file that plays under the whole film" />
					<label class="trim" title="How loud the sound file is next to the shots' own sound">
						Volume
						<input type="range" min="0" max="1" step="0.05" bind:value={project.sound_volume} oninput={saveSoon} aria-label="Volume of the sound file" />
						<span class="pct">{Math.round((project.sound_volume ?? 0.5) * 100)}%</span>
					</label>
				</p>
				<p class="row">
					<label class="trim" title="Music repeats until the film ends. A narration is said once."><input type="checkbox" checked={!project.sound_once} onchange={(e) => ((project.sound_once = !e.currentTarget.checked), saveSoon())} /> Repeat it until the film ends</label>
					<span class="note">It starts with the film and ends with it. You hear it in Play film and in the exported file.</span>
				</p>
				{#if app.voicePackage}
					<h2 class="sub">Narration</h2>
					<textarea bind:value={narration} rows="3" placeholder="Write what is to be said over the film, in English. The voice reads it and the reading becomes the sound under the film." aria-label="Text of the narration" disabled={narrating}></textarea>
					<p class="row">
						<button class="btn" onclick={narrate} disabled={!narration.trim() || narrating}>{narrating ? 'Reading…' : 'Read it aloud'}</button>
						<span class="note">{app.voicePackage.name} reads it on this machine. The reading is saved as narration.wav in the film's folder and takes the place of the sound file above.</span>
					</p>
				{/if}
				<h2 class="sub">Start and end</h2>
				<p class="row">
					<label class="trim"><input type="checkbox" bind:checked={project.fade_in} onchange={saveSoon} /> Fade in at the start</label>
					<label class="trim"><input type="checkbox" bind:checked={project.fade_out} onchange={saveSoon} /> Fade out at the end</label>
					<span class="note">Half a second each, pictures and sound.</span>
				</p>
			</div>
			<div class="panel">
				<h2 class="sub">The exported file</h2>
				<div class="row">
					<span class="label">Size</span>
					<span class="seg" role="radiogroup" aria-label="Size of the exported film">
						<button type="button" role="radio" aria-checked={project.export?.scale !== 2} onclick={() => setExport({ scale: 0 })}>As made, {filmWidth} × {filmHeight}</button>
						<button type="button" role="radio" aria-checked={project.export?.scale === 2} onclick={() => setExport({ scale: 2 })}>Twice as large, {filmWidth * 2} × {filmHeight * 2}</button>
					</span>
					<span class="label">Quality</span>
					<span class="seg" role="radiogroup" aria-label="Quality of the exported film">
						{#each [['small', 'Smaller file'], ['', 'Balanced'], ['best', 'Best']] as [value, label] (value)}
							<button type="button" role="radio" aria-checked={(project.export?.quality ?? '') === value} onclick={() => setExport({ quality: value })}>{label}</button>
						{/each}
					</span>
				</div>
				{#if project.export?.scale === 2}<p class="note">Twice as large only enlarges the picture. It does not add detail, and the file is about two and a half times the size.</p>{/if}
				<p class="row">
					<label class="trim">
						File name
						<input class="fname-in" value={project.export?.name ?? ''} oninput={(e) => setExport({ name: e.currentTarget.value })} placeholder={project.name} aria-label="Name of the exported file" />
						.mp4
					</label>
					<span class="note">24 pictures a second. It goes into the film's folder.</span>
				</p>
				<p class="row">
					<button class="btn" onclick={exportFilm} disabled={plan.parts.length === 0 || exported?.state === 'working'}>{exported?.state === 'working' ? 'Exporting…' : 'Export the film'}</button>
					<label class="trim" title="When the last shot is made, the film is joined into one MP4 file in its folder, also with this screen closed."><input type="checkbox" bind:checked={project.auto_export} onchange={saveSoon} /> Export by itself when the last shot is made</label>
				</p>
			</div>
			{#if video?.credit}<p class="note credit">{video.credit}</p>{/if}
		{/if}
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
		grid-template-columns: minmax(0, 1fr);
		gap: 16px;
		align-content: start;
	}

	.hello {
		display: grid;
		justify-items: center;
		gap: 10px;
		padding: 7vh 0 0;
		text-align: center;
	}

	.quick {
		display: grid;
		gap: 10px;
		padding: 14px;
		border: 1px solid var(--line-soft);
		border-radius: 14px;
	}

	.label {
		font-size: 12.5px;
		color: var(--faint);
	}

	.seg {
		display: inline-flex;
		gap: 2px;
		margin-right: 10px;
	}

	.seg button {
		height: 26px;
		padding: 0 9px;
		border-radius: 8px;
		font-size: 12.5px;
		color: var(--muted);
	}

	.seg button:hover:not(:disabled) {
		background: var(--hover);
	}

	.seg button[aria-checked='true'] {
		background: var(--pressed);
		color: var(--ink);
	}

	.other {
		justify-content: center;
	}

	.credit {
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
		max-width: 100%;
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

	.shot.on {
		border-color: var(--line);
		background: var(--well);
	}

	.shot.making {
		border-color: var(--accent);
	}

	.ask {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 8px;
		padding: 10px 12px;
		border-radius: 10px;
		background: var(--warn-soft);
		font-size: 13.5px;
	}

	.ask span {
		flex: 1 1 320px;
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

	.trim {
		flex: none;
		white-space: nowrap;
		display: inline-flex;
		align-items: center;
		gap: 4px;
		font-size: 12.5px;
		color: var(--faint);
	}

	.trim input[type='checkbox'] {
		width: auto;
		height: auto;
	}

	.trim input {
		width: 52px;
		height: 26px;
		padding: 0 6px;
		border: 1px solid var(--line);
		border-radius: 8px;
		background: var(--bg);
		color: var(--ink);
		font: 400 12.5px/1 var(--rb-text);
	}

	.music {
		flex: 1;
		min-width: 200px;
		height: 30px;
		font-size: 13px;
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
	.poster,
	.blank {
		width: 100%;
		aspect-ratio: var(--shape, 5 / 3);
		max-height: 220px;
		border-radius: 8px;
		background: var(--well);
	}

	.clip {
		display: grid;
		justify-items: center;
		align-content: start;
		gap: 6px;
	}

	.takes {
		display: flex;
		gap: 4px;
		width: 100%;
	}

	.take {
		flex: 1 1 0;
		max-width: 64px;
		aspect-ratio: var(--shape, 5 / 3);
		border-radius: 5px;
		background: var(--well) center / cover no-repeat;
		box-shadow: inset 0 0 0 1px var(--line-soft);
		opacity: 0.7;
	}

	.take:hover {
		opacity: 1;
	}

	.take[aria-pressed='true'] {
		opacity: 1;
		box-shadow:
			inset 0 0 0 2px var(--accent),
			inset 0 0 0 3px var(--bg);
	}

	.takeline {
		display: flex;
		flex-wrap: wrap;
		align-items: center;
		gap: 2px 8px;
		width: 100%;
		font-size: 12px;
		color: var(--faint);
	}

	.takeline label {
		display: inline-flex;
		align-items: center;
		gap: 4px;
	}

	.takeline .tool:last-child {
		margin-left: auto;
	}

	.tool.danger {
		color: var(--fail);
	}

	.poster {
		position: relative;
		display: grid;
		place-items: center;
		background: #000 center / cover no-repeat;
	}

	.poster.stale {
		opacity: 0.45;
	}

	/* A small play mark, shown stronger under the pointer. */
	.go {
		width: 34px;
		height: 34px;
		border-radius: 50%;
		background: color-mix(in srgb, var(--rb-panel) 82%, transparent);
		opacity: 0.75;
		transition: opacity var(--quick);
	}

	.go::after {
		content: '';
		display: block;
		margin: 10px 0 0 13px;
		border-left: 11px solid var(--rb-ink);
		border-top: 7px solid transparent;
		border-bottom: 7px solid transparent;
	}

	.poster:hover .go,
	.poster:focus-visible .go {
		opacity: 1;
	}

	.panel {
		display: grid;
		gap: 8px;
		padding: 12px;
		border: 1px solid var(--line-soft);
		border-radius: 14px;
	}

	.mix {
		gap: 8px 16px;
	}

	.saved {
		flex: 1;
		min-width: 0;
		overflow-wrap: anywhere;
	}

	.trim .fname-in {
		width: 220px;
	}

	.trim select {
		height: 26px;
	}

	.trim input[type='range'] {
		width: 110px;
		padding: 0;
		border: 0;
		accent-color: var(--accent);
	}

	.pct {
		min-width: 4ch;
		font-family: var(--rb-data);
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
