<script lang="ts">
	import * as api from '#lib/api.ts';
	import { bytes as bytesOf } from '#lib/format.ts';
	import { firstWords } from '#lib/studio-look.ts';
	import { tag } from '#lib/studio-tags.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import { shortTime } from '#lib/studio-time.ts';
	import StudioIcon from './StudioIcon.svelte';

	// The Library: the films this install knows, every take of every shot of the open film, and the
	// sound files of the film. Takes are dragged onto their shot, sounds onto a sound track.

	let { onnew }: { onnew: () => void } = $props();

	const tab = $derived(studio.layout.libraryTab);
	const tilde = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');
	const withTakes = $derived(studio.shots.map((s, i) => ({ s, i })).filter(({ s }) => (s.takes?.length ?? 0) > 0));

	let path = $state('');
	let playing = $state('');

	async function addPath() {
		const added = await studio.addSound(path);
		if (added.length > 0) path = '';
	}

	/** One sound file, chosen with the system's dialog. */
	async function addFile() {
		const file = await api.pickFile('sound', 'Choose a sound file');
		if (file) await studio.addSound(file);
	}

	let video = $state('');
	const bringing = $derived(studio.jobs.import?.state === 'working');

	/** A video file, cut into shots at the end of the film. */
	async function bring() {
		const file = api.isTauri ? await api.pickFile('video', 'Choose a video') : video.trim();
		if (file && (await studio.importVideo(file))) video = '';
	}

	async function addFolder() {
		const folder = await api.pickFolder('Choose a folder with sound files');
		if (folder) await studio.addSound(folder);
	}

	async function newEmpty() {
		const folder = await api.pickFolder('Choose a folder for the film');
		if (folder) await studio.openFolder(folder);
		else if (!api.isTauri) onnew();
	}

	async function preview(file: string) {
		const started = await studio.preview(file, () => playing === file && (playing = ''));
		playing = started ? file : '';
	}

	/** A sound goes onto the selected track at the playhead, or onto a new track. */
	function place(file: string) {
		studio.addRegion(studio.trackOf(studio.track)?.id ?? '', file, studio.time);
	}
</script>

<div class="library" role="region" aria-label="Library">
	<div class="st-tabs" role="tablist" aria-label="Library">
		{#each [['films', 'Films', 'Every film here; click one to open it.'], ['takes', 'Takes', 'Every try of every shot, to use another one.'], ['sounds', 'Sounds', 'Sound files of this film, to drag onto a track.']] as [id, label, what] (id)}
			<button role="tab" aria-selected={tab === id} onclick={() => studio.set('libraryTab', id as 'films' | 'takes' | 'sounds')} use:tag={{ name: label, what }}>{label}</button>
		{/each}
	</div>
	<div class="list st-scroll" role="tabpanel">
		{#if tab === 'films'}
			<div class="tools">
				<button class="st-btn" onclick={onnew} use:tag={'new-film'}><StudioIcon name="spark" size={12} /> New film</button>
				<button class="st-btn" onclick={newEmpty} use:tag={'empty-film'}><StudioIcon name="plus" size={12} /> Empty</button>
				<button class="st-btn" onclick={bring} disabled={bringing || (!api.isTauri && !video.trim())} use:tag={'bring-video'}><StudioIcon name="bring" size={12} /> {bringing ? 'Bringing in…' : 'Video'}</button>
			</div>
			{#if !api.isTauri}
				<div class="tools"><input class="st-in grow" bind:value={video} placeholder="/full/path/to/a/video.mp4" aria-label="Full path of a video file to bring in" onkeydown={(e) => (e.stopPropagation(), e.key === 'Enter' && bring())} /></div>
			{/if}
			<ul>
				{#each studio.films as f (f.id)}
					<li class="item film" class:on={f.id === studio.id}>
						<button class="main" onclick={() => studio.open(f.id)} disabled={f.missing} use:tag={{ name: f.name || 'Film', what: f.missing ? `Its folder is gone: ${tilde(f.folder)}` : `Opens it. ${tilde(f.folder)}` }}>
							<StudioIcon name="film" size={13} />
							<span class="name">{f.name || 'Film'}</span>
							<span class="meta">{f.missing ? 'folder gone' : `${f.made ?? 0}/${f.shots ?? 0}`}</span>
						</button>
						<button
							class="x"
							onclick={() => (studio.question = { text: `Take "${f.name || 'Film'}" out of the list? Its folder and files stay where they are.`, yes: 'Remove from list', no: 'Keep', act: () => studio.forget(f.id) })}
							use:tag={'remove-film'}
							aria-label="Remove {f.name || 'Film'} from the list"><StudioIcon name="close" size={10} /></button
						>
					</li>
				{/each}
			</ul>
		{:else if tab === 'takes'}
			{#if withTakes.length === 0}
				<p class="st-note pad">No takes yet. Make a shot, then make it again: every try is kept as a take, up to four.</p>
			{/if}
			{#each withTakes as { s, i } (s.id)}
				<div class="group" class:on={studio.shotsSel.includes(s.id)}>
					<button class="ghead" onclick={() => studio.selectShot(s.id)}>
						<span class="n">{i + 1}</span>
						<span class="name">{firstWords(s.prompt, 5) || 'No description'}</span>
					</button>
					<div class="takes" style:--shape={(studio.doc?.size ?? '640x384').replace('x', ' / ')}>
						{#each s.takes ?? [] as t, k (t.n)}
							<button
								class="take"
								draggable="true"
								aria-pressed={t.n === s.take}
								style:background-image={studio.takeUrls[`${s.id}.${t.n}`] ? `url(${studio.takeUrls[`${s.id}.${t.n}`]})` : undefined}
								ondragstart={(e) => {
									e.dataTransfer?.setData(`application/x-rigbrew-take-${s.id}`, String(t.n));
									if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy';
								}}
								onclick={() => studio.selectShot(s.id)}
								ondblclick={() => studio.useTake(s.id, t.n)}
								use:tag={{ id: 'take', name: `Take ${k + 1} of shot ${i + 1}`, what: t.n === s.take ? 'In use in the film.' : 'Drag it onto its shot, or double-click, to use it.' }}
								aria-label="Take {k + 1} of shot {i + 1}{t.n === s.take ? ', in use' : ''}"
							>
								<span>{k + 1}</span>
							</button>
						{/each}
					</div>
				</div>
			{/each}
		{:else}
			<div class="tools col" class:gone={api.isTauri && studio.sounds.length === 0}>
				{#if api.isTauri}
					<div class="st-row">
						<button class="st-btn" onclick={addFile} disabled={studio.busy !== ''} use:tag={'add-sound'}><StudioIcon name="plus" size={12} /> Sound file</button>
						<button class="st-btn" onclick={addFolder} disabled={studio.busy !== ''} use:tag={'add-folder'}><StudioIcon name="folder" size={12} /> Folder</button>
					</div>
				{:else}
					<div class="st-row">
						<input class="st-in grow" bind:value={path} placeholder="/full/path/to/a/sound/file" aria-label="Full path of a sound file or a folder" onkeydown={(e) => (e.stopPropagation(), e.key === 'Enter' && addPath())} />
						<button class="st-btn" onclick={addPath} disabled={!path.trim() || studio.busy !== ''}>Add</button>
					</div>
				{/if}
			</div>
			{#if studio.sounds.length === 0}
				<div class="empty">
					<StudioIcon name="wave" size={20} />
					<p>No sounds yet. Add music or effects, then drag them onto a track.</p>
					{#if api.isTauri}
						<button class="st-btn primary" onclick={addFile} disabled={studio.busy !== ''}><StudioIcon name="plus" size={12} /> Add a sound file</button>
						<button class="st-btn quiet" onclick={addFolder} disabled={studio.busy !== ''} use:tag={'add-folder'}><StudioIcon name="folder" size={12} /> Or a folder of them</button>
					{/if}
				</div>
			{/if}
			<ul>
				{#each studio.sounds as snd (snd.file)}
					{@const seconds = (studio.drawn, studio.fileSeconds(snd.file))}
					<li
						class="item sound"
						class:missing={snd.missing}
						draggable={!snd.missing}
						ondragstart={(e) => {
							e.dataTransfer?.setData('application/x-rigbrew-sound', snd.file);
							if (e.dataTransfer) e.dataTransfer.effectAllowed = 'copy';
						}}
					>
						<button class="play" onclick={() => preview(snd.file)} disabled={snd.missing} aria-label="{playing === snd.file ? 'Stop' : 'Play'} {snd.name}" use:tag={{ id: 'listen', name: playing === snd.file ? 'Stop' : 'Listen' }}>
							<StudioIcon name={playing === snd.file ? 'stop' : 'play'} size={10} />
						</button>
						<button class="main" onclick={() => preview(snd.file)} ondblclick={() => place(snd.file)} disabled={snd.missing} use:tag={{ name: snd.name, what: snd.missing ? `Not found: ${tilde(snd.file)}` : 'Drag it onto a sound track, or double-click to set it at the playhead.' }}>
							<span class="name">{snd.name}</span>
							<span class="meta">{snd.missing ? 'not found' : seconds ? shortTime(seconds) : snd.bytes < 1048576 ? `${Math.max(1, Math.round(snd.bytes / 1024))} KB` : bytesOf(snd.bytes)}</span>
						</button>
						{#if snd.from === 'added'}
							<button class="x" onclick={() => studio.removeSound(snd.file)} use:tag={{ name: 'Remove from list', what: 'The file stays where it is.' }} aria-label="Remove {snd.name} from the list"><StudioIcon name="close" size={10} /></button>
						{/if}
					</li>
				{/each}
			</ul>
		{/if}
	</div>
</div>

<style>
	.library {
		height: 100%;
		min-width: 0;
		display: flex;
		flex-direction: column;
		background: var(--st-panel);
	}

	.list {
		flex: 1 1 0;
		min-height: 0;
	}

	.pad {
		padding: 8px;
	}

	.tools {
		display: flex;
		flex-wrap: wrap;
		gap: 4px;
		padding: 6px;
		border-bottom: 1px solid var(--st-edge);
	}

	.tools.col {
		flex-direction: column;
		align-items: stretch;
	}

	.tools.gone {
		display: none;
	}

	/* No sounds yet: a line and the button that adds the first. */
	.empty {
		display: grid;
		justify-items: center;
		gap: 8px;
		padding: 22px 12px;
		text-align: center;
		color: var(--st-accent);
	}

	.empty p {
		max-width: 24ch;
		margin: 0;
		font-size: 11.5px;
		line-height: 1.4;
		color: var(--st-ink);
	}

	ul {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.item {
		display: flex;
		align-items: center;
		height: 24px;
		border-bottom: 1px solid rgba(0, 0, 0, 0.35);
	}

	.item:hover {
		background: rgba(255, 255, 255, 0.04);
	}

	.item.on {
		background: color-mix(in srgb, var(--st-accent) 24%, transparent);
	}

	.item.missing .name {
		color: var(--st-fail);
	}

	.main {
		flex: 1;
		min-width: 0;
		height: 100%;
		display: flex;
		align-items: center;
		gap: 6px;
		padding: 0 7px;
		text-align: left;
		color: var(--st-muted);
	}

	.sound .main {
		padding-left: 2px;
		cursor: grab;
	}

	.name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 11.5px;
		color: var(--st-ink);
	}

	.meta {
		flex: none;
		font: 500 10px/1 var(--rb-data);
		font-variant-numeric: tabular-nums;
		color: var(--st-faint);
	}

	.x,
	.play {
		flex: none;
		width: 20px;
		height: 100%;
		display: grid;
		place-items: center;
		color: var(--st-faint);
	}

	.x {
		visibility: hidden;
	}

	.item:hover .x,
	.x:focus-visible {
		visibility: visible;
	}

	.x:hover,
	.play:hover {
		color: var(--st-ink);
	}

	.group {
		padding: 0 6px 7px;
		border-bottom: 1px solid rgba(0, 0, 0, 0.35);
	}

	.group.on {
		background: rgba(255, 255, 255, 0.04);
	}

	.ghead {
		width: 100%;
		height: 24px;
		display: flex;
		align-items: center;
		gap: 6px;
		text-align: left;
	}

	.n {
		flex: none;
		min-width: 14px;
		font: 600 10.5px/1 var(--rb-data);
		color: var(--st-muted);
	}

	.takes {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 3px;
	}

	.take {
		position: relative;
		aspect-ratio: var(--shape, 5 / 3);
		border-radius: 2px;
		background: #000 center / cover no-repeat;
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1);
		opacity: 0.6;
		cursor: grab;
	}

	.take:hover {
		opacity: 1;
	}

	.take[aria-pressed='true'] {
		opacity: 1;
		box-shadow:
			inset 0 0 0 1.5px #fff,
			inset 0 0 0 2.5px #000;
	}

	.take span {
		position: absolute;
		left: 2px;
		bottom: 2px;
		padding: 0 3px;
		border-radius: 2px;
		background: rgba(0, 0, 0, 0.7);
		font: 600 9px/12px var(--rb-data);
		color: #fff;
	}
</style>
