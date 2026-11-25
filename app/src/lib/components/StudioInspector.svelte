<script lang="ts">
	import { videoLengthsOf, videoSizesOf, type SizeName } from '#lib/chat.svelte.ts';
	import { bytes as bytesOf } from '#lib/format.ts';
	import { app } from '#lib/state.svelte.ts';
	import { dbLabel, panLabel, regionEnd, regionSpan, type DocRegion, type DocShot, type DocTrack } from '#lib/studio-edit.ts';
	import { STATE_WORD, fileName } from '#lib/studio-look.ts';
	import { SHOTS, TRACK_COLORS, studio } from '#lib/studio.svelte.ts';
	import { shortTime, timecode } from '#lib/studio-time.ts';
	import { hasClip, roughly, shotSeconds } from '#lib/studio.ts';
	import { isMac } from '#lib/words.ts';
	import StudioFader from './StudioFader.svelte';
	import StudioIcon from './StudioIcon.svelte';
	import StudioKnob from './StudioKnob.svelte';
	import StudioTime from './StudioTime.svelte';

	// The Inspector: everything about what is selected. One shot, one region or one track; with
	// nothing selected, the film itself and the tools that write shots and read narration.

	const video = $derived(app.videoPackage);
	const lengths = $derived(videoLengthsOf(video));
	const shapes = $derived(videoSizesOf(video));
	const doc = $derived(studio.doc);
	const shot = $derived(studio.shotsSel.length === 1 ? studio.shot(studio.shotsSel[0]) : undefined);
	const shotAt = $derived(shot ? studio.shots.findIndex((s) => s.id === shot.id) : -1);
	const found = $derived(studio.regionsSel.length === 1 ? studio.region(studio.regionsSel[0]) : null);
	const track = $derived(studio.trackSel && studio.regionsSel.length === 0 && studio.shotsSel.length === 0 ? (studio.track === SHOTS ? null : studio.trackOf(studio.track)) : undefined);
	const strip = $derived(studio.trackSel && studio.track === SHOTS && studio.regionsSel.length === 0 && studio.shotsSel.length === 0);
	const tilde = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');
	const clock = (s: number) => (s >= 60 ? `${Math.floor(s / 60)} min ${Math.round(s % 60)} s` : `${Math.round(s)} s`);

	/** One change to the selected shot, as one step for undo. */
	function setShot(label: string, change: (s: DocShot) => void, key = '') {
		const id = shot?.id;
		if (!id) return;
		studio.edit(
			label,
			() => {
				const s = studio.docShot(id);
				if (s) change(s);
			},
			key ? `${key}:${id}` : ''
		);
	}

	function setRegion(label: string, change: (r: DocRegion) => DocRegion | void, key = '') {
		const id = found?.region.id;
		if (!id) return;
		studio.edit(
			label,
			(d) => {
				for (const t of d.tracks) {
					const at = t.regions.findIndex((r) => r.id === id);
					if (at < 0) continue;
					const next = change(t.regions[at]);
					if (next) t.regions[at] = next;
				}
			},
			key ? `${key}:${id}` : ''
		);
	}

	function setTrack(label: string, change: (t: DocTrack) => void, key = '') {
		const id = track?.id;
		if (!id) return;
		studio.edit(
			label,
			() => {
				const t = studio.trackOf(id);
				if (t) change(t);
			},
			key ? `${key}:${id}` : ''
		);
	}

	const liveShot = (change: (s: DocShot) => void) => studio.live(() => shot && change(studio.docShot(shot.id)!));
	const liveRegion = (change: (r: DocRegion) => void) => studio.live(() => found && change(studio.region(found.region.id)!.region));
	const liveTrack = (change: (t: DocTrack) => void) => studio.live(() => track && change(studio.trackOf(track.id)!));

	// What a shot of this length takes here, from the shots made on this machine.
	const estimate = $derived.by(() => {
		const pace = studio.pace;
		if (!shot || !pace) return 0;
		return (shot.continues && shotAt > 0 ? pace.continuing : pace.plain) * (shot.frames / pace.frames);
	});
	const used = $derived(shot?.takes?.find((t) => t.n === shot.take));

	let hint = $state('');
	let idea = $state('');
	let wanted = $state(6);
	let narration = $state('');

	async function write(replace: boolean) {
		if (await studio.write(idea, wanted, replace)) idea = '';
	}

	function replaceShots() {
		const made = studio.shots.filter(hasClip).length;
		if (made === 0) return write(true);
		studio.question = {
			text: `The film has ${made} made ${made === 1 ? 'shot' : 'shots'}. New shots take the place of all of them. Undo brings them back with their clips.`,
			yes: 'Replace the shots',
			no: 'Keep them',
			act: () => write(true)
		};
	}

	async function narrate() {
		if (await studio.narrate(narration)) narration = '';
	}

	function setShape(name: SizeName) {
		const size = `${shapes[name].width}x${shapes[name].height}`;
		if (!doc || doc.size === size) return;
		const made = studio.shots.filter((s) => s.state === 'done').length;
		const act = () => studio.edit('Shape of the film', (d) => (d.size = size));
		if (made === 0) act();
		else studio.question = { text: `Every shot of a film has the same shape. The ${made} made ${made === 1 ? 'shot has' : 'shots have'} to be made again in the new one.`, yes: 'Change the shape', no: 'Keep it', act };
	}
	const [filmWidth, filmHeight] = $derived((doc?.size ?? '640x384').split('x').map(Number));
</script>

{#snippet narrationTool(where: string)}
	{#if app.voicePackage}
		<section class="st-sec">
			<h3 class="st-h">Narration</h3>
			<textarea class="st-ta" bind:value={narration} rows="3" placeholder="What is said over the film, in English" aria-label="Text of the narration" disabled={studio.busy !== ''} onkeydown={(e) => e.stopPropagation()}></textarea>
			<div class="st-row">
				<button class="st-btn" onclick={narrate} disabled={!narration.trim() || studio.busy !== ''}>{studio.busy === 'narrating' ? 'Reading…' : 'Read it aloud'}</button>
				<span class="st-note">{app.voicePackage.name} reads it. It becomes a region at the playhead on {where}.</span>
			</div>
		</section>
	{/if}
{/snippet}

<div class="inspector st-scroll" role="region" aria-label="Inspector">
	{#if !doc}
		<p class="st-note pad">No film is open.</p>
	{:else if shot}
		{@const made = hasClip(shot)}
		<header class="top">
			<span class="kind">Shot {shotAt + 1}</span>
			<span class="state {shot.state}">{STATE_WORD[shot.state]}</span>
			<span class="grow"></span>
			{#if shot.prompt.trim() && video}
				<button class="st-btn" onclick={() => studio.make([shot.id])} disabled={shot.state === 'making' || shot.state === 'queued'} title={made ? 'Makes this shot again. The clip it has is kept as a take.' : 'Make this shot'}>{made ? 'Try again' : 'Make'}</button>
			{/if}
		</header>
		{#if shot.error}<p class="st-problem selectable" role="alert">{shot.error}</p>{/if}
		<section class="st-sec">
			<h3 class="st-h">Description</h3>
			<textarea
				class="st-ta big"
				rows="7"
				value={shot.prompt}
				placeholder="What is seen, what moves, the place, the light, the camera, the sound"
				aria-label="Description of shot {shotAt + 1}"
				oninput={(e) => setShot('Description', (s) => (s.prompt = e.currentTarget.value), 'prompt')}
				onkeydown={(e) => e.stopPropagation()}
			></textarea>
			<div class="st-row">
				<input class="st-in grow" bind:value={hint} placeholder="What should be different (optional)" aria-label="What the chat model should change" onkeydown={(e) => (e.stopPropagation(), e.key === 'Enter' && studio.rewrite(shot.id, hint))} />
				<button class="st-btn" onclick={() => studio.rewrite(shot.id, hint)} disabled={studio.busy !== '' || app.chatPackages.length === 0} title="The chat model writes this description again, in the words of the shots around it. Undo brings the old one back.">
					<StudioIcon name="spark" size={12} />
					{studio.busy === 'rewriting' ? 'Writing…' : 'Rewrite'}
				</button>
			</div>
			{#if app.chatPackages.length === 0}<p class="st-note">Rewriting needs a chat model.</p>{/if}
		</section>
		<section class="st-sec">
			<h3 class="st-h">Shot</h3>
			<div class="st-row">
				<span class="st-lab">Length</span>
				<span class="st-seg" role="radiogroup" aria-label="Length of the shot">
					{#each lengths as l (l.frames)}
						<button role="radio" aria-checked={shot.frames === l.frames} onclick={() => setShot('Length', (s) => (s.frames = l.frames))} title={made ? 'A made shot is marked Changed until it is made again. Its clip stays as a take.' : ''}>{l.label}</button>
					{/each}
				</span>
			</div>
			{#if shotAt > 0 && studio.view?.can_continue}
				<label class="st-row st-check" title="The shot starts from the last picture of the shot before it, so the two join without a cut. It takes longer to make.">
					<input type="checkbox" checked={shot.continues} onchange={(e) => setShot('Continue the shot before', (s) => (s.continues = e.currentTarget.checked))} />
					Continues the shot before
				</label>
			{/if}
			{#if shotAt > 0 && !shot.continues}
				<div class="st-row">
					<span class="st-lab">Join</span>
					<span class="st-seg" role="radiogroup" aria-label="How the film goes into this shot">
						{#each [['cut', 'Cut'], ['dissolve', 'Dissolve'], ['black', 'Fade']] as [value, label] (value)}
							<button role="radio" aria-checked={(shot.join || 'cut') === value} onclick={() => setShot('Join', (s) => (s.join = value as DocShot['join']))} title={value === 'black' ? 'A fade through black' : ''}>{label}</button>
						{/each}
					</span>
					{#if shot.join === 'dissolve' || shot.join === 'black'}
						<StudioTime value={shot.join_seconds ?? 0.5} label="Length of the join" min={0.25} max={2} onset={(v) => setShot('Join length', (s) => (s.join_seconds = v))} />
					{/if}
				</div>
			{/if}
			<div class="st-row">
				<span class="st-lab">Seed</span>
				<input
					class="st-in num"
					type="number"
					min="0"
					max="4294967295"
					value={shot.seed ?? ''}
					placeholder="new each time"
					aria-label="Seed for the next clip"
					onchange={(e) => setShot('Seed', (s) => (s.seed = e.currentTarget.value === '' ? null : Math.max(0, Math.floor(Number(e.currentTarget.value)))))}
					onkeydown={(e) => e.stopPropagation()}
				/>
				{#if used?.seed !== undefined}
					<label class="st-check" title="With the same seed and the same description the video model makes the same clip again. Keep the seed and change the description a little to change the clip a little.">
						<input type="checkbox" checked={shot.seed === used.seed} onchange={(e) => setShot('Seed', (s) => (s.seed = e.currentTarget.checked ? (used.seed ?? null) : null))} />
						Keep its seed
					</label>
				{/if}
			</div>
		</section>
		{#if made}
			{@const whole = shotSeconds(shot)}
			<section class="st-sec">
				<h3 class="st-h">Trim</h3>
				<div class="st-row">
					<span class="st-lab">Cut at start</span>
					<StudioTime value={shot.trim_start ?? 0} label="Cut from the start" max={Math.max(0, whole - 0.5 - (shot.trim_end ?? 0))} onset={(v) => setShot('Trim shot', (s) => (s.trim_start = v))} />
					<span class="st-lab right">at end</span>
					<StudioTime value={shot.trim_end ?? 0} label="Cut from the end" max={Math.max(0, whole - 0.5 - (shot.trim_start ?? 0))} onset={(v) => setShot('Trim shot', (s) => (s.trim_end = v))} />
				</div>
				<p class="st-note">Plays {shortTime(whole - (shot.trim_start ?? 0) - (shot.trim_end ?? 0))} of {shortTime(whole)}.</p>
			</section>
			<section class="st-sec">
				<h3 class="st-h">Sound of the shot</h3>
				<div class="st-row">
					<button class="st-ms m" class:lit={shot.mute} aria-pressed={shot.mute} onclick={() => setShot('Mute shot', (s) => (s.mute = !s.mute))} title="Mute this shot's own sound">M</button>
					<StudioFader
						value={shot.volume ?? 1}
						top={1.5}
						disabled={shot.mute}
						label="Volume of the shot"
						onstart={() => studio.begin()}
						onchange={(v) => liveShot((s) => (s.volume = Math.round(v * 100) / 100))}
						onend={() => studio.commit('Shot volume')}
						onset={(v) => setShot('Shot volume', (s) => (s.volume = Math.round(v * 100) / 100), 'vol')}
					/>
					<span class="st-val">{shot.mute ? 'off' : `${dbLabel(shot.volume ?? 1)} dB`}</span>
				</div>
			</section>
		{/if}
		{#if (shot.takes?.length ?? 0) > 0}
			<section class="st-sec">
				<h3 class="st-h">Takes <span class="count">{shot.takes!.length}</span></h3>
				<div class="takes" style:--shape={doc.size.replace('x', ' / ')}>
					{#each shot.takes! as t, i (t.n)}
						<button
							class="take"
							aria-pressed={t.n === shot.take}
							onclick={() => studio.useTake(shot.id, t.n)}
							style:background-image={studio.takeUrls[`${shot.id}.${t.n}`] ? `url(${studio.takeUrls[`${shot.id}.${t.n}`]})` : undefined}
							title="Take {i + 1}{t.seconds ? `, made in ${clock(t.seconds)}` : ''}{t.n === shot.take ? ', in use' : '. Click to use it.'}"
							aria-label="Take {i + 1}{t.n === shot.take ? ', in use' : ''}"
						>
							<span>{i + 1}</span>
						</button>
					{/each}
				</div>
				<div class="st-row">
					<button class="st-btn" onclick={() => studio.make([shot.id])} disabled={!video || shot.state === 'making' || shot.state === 'queued' || !shot.prompt.trim()}>Try again</button>
					{#if shot.take}
						<button class="st-btn quiet" onclick={() => studio.deleteTake(shot.id, shot.take!)} disabled={shot.state === 'making' || shot.state === 'queued'} title="Delete the take in use and its clip"><StudioIcon name="trash" size={12} /> Delete take</button>
					{/if}
				</div>
			</section>
		{/if}
		<section class="st-sec">
			<h3 class="st-h">Time to make</h3>
			<dl class="facts">
				{#if video && estimate > 0}
					<dt>Estimated</dt>
					<dd>{roughly(estimate)}{studio.pace?.measured ? '' : ', as a first guess'}</dd>
				{/if}
				{#if shot.seconds}
					<dt>Measured</dt>
					<dd>{clock(shot.seconds)}</dd>
				{/if}
				{#if made && shot.made_frames}
					<dt>Clip</dt>
					<dd>{shot.made_frames} frames, {shortTime(shot.made_frames / 24)}</dd>
				{/if}
			</dl>
		</section>
	{:else if studio.shotsSel.length > 1}
		{@const picked = studio.shots.filter((s) => studio.shotsSel.includes(s.id))}
		<header class="top"><span class="kind">{picked.length} shots</span></header>
		<section class="st-sec">
			<p class="st-note">{picked.filter(hasClip).length} made. Together {shortTime(picked.reduce((n, s) => n + shotSeconds(s), 0))}.</p>
			<div class="st-row">
				<button class="st-btn" onclick={() => studio.make(picked.filter((s) => s.prompt.trim()).map((s) => s.id))} disabled={!video}>Make these</button>
				<button class="st-btn" onclick={() => studio.duplicateSelection()}>Duplicate</button>
				<button class="st-btn quiet" onclick={() => studio.deleteSelection()}><StudioIcon name="trash" size={12} /> Delete</button>
			</div>
		</section>
	{:else if found}
		{@const r = found.region}
		{@const file = (studio.drawn, studio.fileSeconds(r.file))}
		<header class="top">
			<span class="chip" style:background={found.track.color}></span>
			<span class="kind">Region</span>
			<span class="state">{found.track.name}</span>
		</header>
		<section class="st-sec">
			<h3 class="st-h">File</h3>
			<p class="path selectable" title={r.file}>{fileName(r.file)}</p>
			<p class="st-note">{file ? `${shortTime(file)} long. ` : ''}{tilde(r.file)}</p>
			{#if studio.overlaps.has(r.id)}<p class="st-note warn">It lies over another region on this track. Both are heard.</p>{/if}
		</section>
		<section class="st-sec">
			<h3 class="st-h">Place</h3>
			<div class="st-row">
				<span class="st-lab">Start</span>
				<StudioTime full value={r.start} label="Start in the film" onset={(v) => setRegion('Move region', (x) => void (x.start = v))} />
			</div>
			<div class="st-row">
				<span class="st-lab">In point</span>
				<StudioTime value={r.offset} label="Where in the file it starts" max={Math.max(0, (file ?? Infinity) - 2 / 24)} onset={(v) => setRegion('In point', (x) => void ((x.length = Math.max(2 / 24, x.length + x.offset - v)), (x.offset = v)))} />
				<span class="st-lab right">Out</span>
				<StudioTime value={r.offset + r.length} label="Where in the file it stops" min={r.offset + 2 / 24} max={file ?? Infinity} onset={(v) => setRegion('Out point', (x) => void (x.length = Math.max(2 / 24, v - x.offset)))} />
			</div>
			<div class="st-row">
				<span class="st-lab">Length</span>
				<StudioTime value={r.length} label="Length of the region" min={2 / 24} max={(file ?? Infinity) - r.offset} onset={(v) => setRegion('Length', (x) => void (x.length = v))} />
				<span class="st-lab right">Plays</span>
				<input
					class="st-in tiny"
					type="number"
					min="1"
					max="1000"
					value={r.loops}
					aria-label="How many times the region plays"
					title="How many times it plays, one after another"
					onchange={(e) => setRegion('Repeats', (x) => void (x.loops = Math.min(1000, Math.max(1, Math.floor(Number(e.currentTarget.value) || 1)))))}
					onkeydown={(e) => e.stopPropagation()}
				/>
				<span class="st-val left">{r.loops === 1 ? 'time' : 'times'}</span>
			</div>
			<p class="st-note">Ends at {timecode(regionEnd(r))}.</p>
		</section>
		<section class="st-sec">
			<h3 class="st-h">Level</h3>
			<div class="st-row">
				<span class="st-lab">Gain</span>
				<StudioFader
					value={r.gain}
					label="Gain of the region"
					onstart={() => studio.begin()}
					onchange={(v) => liveRegion((x) => (x.gain = Math.round(v * 1000) / 1000))}
					onend={() => studio.commit('Region gain')}
					onset={(v) => setRegion('Region gain', (x) => void (x.gain = Math.round(v * 1000) / 1000), 'gain')}
				/>
				<span class="st-val">{dbLabel(r.gain)} dB</span>
			</div>
			<div class="st-row">
				<span class="st-lab">Fade in</span>
				<StudioTime value={r.fade_in} label="Fade in" max={regionSpan(r) - r.fade_out} onset={(v) => setRegion('Fade', (x) => void (x.fade_in = v))} />
				<span class="st-lab right">Fade out</span>
				<StudioTime value={r.fade_out} label="Fade out" max={regionSpan(r) - r.fade_in} onset={(v) => setRegion('Fade', (x) => void (x.fade_out = v))} />
			</div>
		</section>
		<section class="st-sec">
			<div class="st-row">
				<button class="st-btn" onclick={() => studio.splitAtPlayhead()} title="Cut it in two at the playhead"><StudioIcon name="split" size={12} /> Split</button>
				<button class="st-btn" onclick={() => studio.duplicateSelection()}>Duplicate</button>
				<button class="st-btn quiet" onclick={() => studio.deleteSelection()}><StudioIcon name="trash" size={12} /> Delete</button>
			</div>
		</section>
	{:else if studio.regionsSel.length > 1}
		<header class="top"><span class="kind">{studio.regionsSel.length} regions</span></header>
		<section class="st-sec">
			<div class="st-row">
				<button class="st-btn" onclick={() => studio.splitAtPlayhead()}><StudioIcon name="split" size={12} /> Split</button>
				<button class="st-btn" onclick={() => studio.duplicateSelection()}>Duplicate</button>
				<button class="st-btn quiet" onclick={() => studio.deleteSelection()}><StudioIcon name="trash" size={12} /> Delete</button>
			</div>
		</section>
	{:else if track}
		<header class="top">
			<span class="chip" style:background={track.color}></span>
			<span class="kind">Track</span>
			<span class="state">{track.regions.length} {track.regions.length === 1 ? 'region' : 'regions'}</span>
		</header>
		<section class="st-sec">
			<div class="st-row">
				<span class="st-lab">Name</span>
				<input class="st-in grow" value={track.name} maxlength="40" aria-label="Name of the track" oninput={(e) => setTrack('Rename track', (t) => (t.name = e.currentTarget.value), 'name')} onkeydown={(e) => e.stopPropagation()} />
			</div>
			<div class="st-row">
				<span class="st-lab">Colour</span>
				<span class="swatches" role="radiogroup" aria-label="Colour of the track">
					{#each TRACK_COLORS as c (c)}
						<button role="radio" aria-checked={track.color === c} aria-label="Colour {c}" style:background={c} onclick={() => setTrack('Track colour', (t) => (t.color = c))}></button>
					{/each}
				</span>
			</div>
			<div class="st-row">
				<span class="st-lab">Volume</span>
				<StudioFader
					value={track.volume}
					label="Volume of the track"
					onstart={() => studio.begin()}
					onchange={(v) => liveTrack((t) => (t.volume = v))}
					onend={() => studio.commit('Volume')}
					onset={(v) => setTrack('Volume', (t) => (t.volume = v), 'vol')}
				/>
				<span class="st-val">{dbLabel(track.volume)} dB</span>
			</div>
			<div class="st-row">
				<span class="st-lab">Pan</span>
				<StudioKnob
					value={track.pan}
					size={22}
					label="Pan of the track"
					onstart={() => studio.begin()}
					onchange={(v) => liveTrack((t) => (t.pan = v))}
					onend={() => studio.commit('Pan')}
					onset={(v) => setTrack('Pan', (t) => (t.pan = v), 'pan')}
				/>
				<span class="st-val left">{panLabel(track.pan)}</span>
				<span class="grow"></span>
				<button class="st-ms m" class:lit={track.mute} aria-pressed={track.mute} onclick={() => studio.toggle(track.id, 'mute')} title="Mute (M)">M</button>
				<button class="st-ms s" class:lit={track.solo} aria-pressed={track.solo} onclick={() => studio.toggle(track.id, 'solo')} title="Solo (S)">S</button>
			</div>
			{#if !studio.heard[track.id]}<p class="st-note">{track.mute ? 'Muted: it is not in the exported film.' : 'Another track is soloed: this one is not heard, and not in the exported film.'}</p>{/if}
		</section>
		<section class="st-sec">
			<div class="st-row">
				<button class="st-btn" onclick={() => studio.duplicateTrack(track.id)}>Duplicate track</button>
				<button class="st-btn quiet" onclick={() => studio.deleteTrack(track.id)}><StudioIcon name="trash" size={12} /> Delete track</button>
			</div>
		</section>
		{@render narrationTool('this track when it is the narration track, else on the track named Narration')}
	{:else if strip}
		{@const s = doc.shot_track}
		<header class="top">
			<span class="kind">Shot sound</span>
			<span class="state">the sound the shots were made with</span>
		</header>
		<section class="st-sec">
			<div class="st-row">
				<span class="st-lab">Volume</span>
				<StudioFader
					value={s.volume}
					label="Volume of the shot sound"
					onstart={() => studio.begin()}
					onchange={(v) => studio.live((d) => (d.shot_track.volume = v))}
					onend={() => studio.commit('Volume')}
					onset={(v) => studio.edit('Volume', (d) => (d.shot_track.volume = v), 'vol:shots')}
				/>
				<span class="st-val">{dbLabel(s.volume)} dB</span>
			</div>
			<div class="st-row">
				<span class="st-lab">Pan</span>
				<StudioKnob
					value={s.pan}
					size={22}
					label="Pan of the shot sound"
					onstart={() => studio.begin()}
					onchange={(v) => studio.live((d) => (d.shot_track.pan = v))}
					onend={() => studio.commit('Pan')}
					onset={(v) => studio.edit('Pan', (d) => (d.shot_track.pan = v), 'pan:shots')}
				/>
				<span class="st-val left">{panLabel(s.pan)}</span>
				<span class="grow"></span>
				<button class="st-ms m" class:lit={s.mute} aria-pressed={s.mute} onclick={() => studio.toggle(SHOTS, 'mute')} title="Mute (M)">M</button>
				<button class="st-ms s" class:lit={s.solo} aria-pressed={s.solo} onclick={() => studio.toggle(SHOTS, 'solo')} title="Solo (S)">S</button>
			</div>
			<p class="st-note">Each shot has its own volume too: drag the line on its sound, or select the shot.</p>
		</section>
	{:else}
		<header class="top">
			<span class="kind">Film</span>
			<span class="state">{doc.shots.length} {doc.shots.length === 1 ? 'shot' : 'shots'}, {timecode(studio.plan.total)}</span>
		</header>
		<section class="st-sec">
			<div class="st-row">
				<span class="st-lab">Name</span>
				<input class="st-in grow" value={doc.name} maxlength="80" aria-label="Name of the film" oninput={(e) => studio.edit('Name of the film', (d) => (d.name = e.currentTarget.value), 'film-name')} onkeydown={(e) => e.stopPropagation()} />
			</div>
			<div class="st-row">
				<span class="st-lab">Shape</span>
				<span class="st-seg shapes" role="radiogroup" aria-label="Shape of the film">
					{#each Object.entries(shapes) as [name, s] (name)}
						<button role="radio" aria-checked={doc.size === `${s.width}x${s.height}`} onclick={() => setShape(name as SizeName)} disabled={studio.view?.rendering} title="{s.label}, {s.width} × {s.height}" aria-label={s.label}><span class="shape" style:aspect-ratio="{s.width} / {s.height}"></span></button>
					{/each}
				</span>
				<span class="st-val left">{filmWidth} × {filmHeight}</span>
			</div>
			<div class="st-row">
				<span class="st-lab">Folder</span>
				<span class="path one selectable" title={studio.project?.folder}>{tilde(studio.project?.folder ?? '')}</span>
				<button class="st-btn quiet icon" onclick={() => studio.reveal()} title={isMac ? 'Show in Finder' : 'Show in folder'} aria-label={isMac ? 'Show in Finder' : 'Show in folder'}><StudioIcon name="folder" size={12} /></button>
			</div>
			<dl class="facts">
				<dt>Length</dt>
				<dd>{timecode(studio.plan.total)}{studio.exportPlan.total !== studio.plan.total ? `, ${timecode(studio.exportPlan.total)} made` : ''}</dd>
				<dt>Made</dt>
				<dd>{studio.shots.filter((s) => s.state === 'done').length} of {doc.shots.length}{studio.todo > 0 && video && studio.view?.estimate ? `. The rest takes ${roughly(studio.view.estimate.all)}.` : ''}</dd>
				<dt>Sound</dt>
				<dd>{doc.tracks.length} {doc.tracks.length === 1 ? 'track' : 'tracks'}, {doc.tracks.reduce((n, t) => n + t.regions.length, 0)} regions</dd>
			</dl>
		</section>
		<section class="st-sec">
			<h3 class="st-h">Start and end</h3>
			<div class="st-row">
				<label class="st-check"><input type="checkbox" checked={doc.fade_in} onchange={(e) => studio.edit('Fade in', (d) => (d.fade_in = e.currentTarget.checked))} /> Fade in</label>
				<label class="st-check"><input type="checkbox" checked={doc.fade_out} onchange={(e) => studio.edit('Fade out', (d) => (d.fade_out = e.currentTarget.checked))} /> Fade out</label>
				<span class="st-note">Half a second, picture and sound.</span>
			</div>
		</section>
		<section class="st-sec">
			<h3 class="st-h">Write the shots from an idea</h3>
			<textarea class="st-ta" bind:value={idea} rows="3" placeholder="What is the film about? For example: a red fox finds a paper boat in a snowy forest and follows it down a stream" aria-label="What the film is about" disabled={studio.busy !== ''} onkeydown={(e) => e.stopPropagation()}></textarea>
			<div class="st-row">
				<select class="st-in" bind:value={wanted} aria-label="How many shots">{#each [3, 4, 6, 8, 12, 16, 24, 36, 60] as n (n)}<option value={n}>{n} shots</option>{/each}</select>
				<button class="st-btn" onclick={() => write(false)} disabled={!idea.trim() || studio.busy !== '' || app.chatPackages.length === 0}>{studio.busy === 'writing' ? 'Writing…' : doc.shots.length ? 'Add to the film' : 'Write the shots'}</button>
				{#if doc.shots.length > 0}<button class="st-btn quiet" onclick={replaceShots} disabled={!idea.trim() || studio.busy !== '' || app.chatPackages.length === 0}>Replace the shots</button>{/if}
			</div>
			{#if app.chatPackages.length === 0}<p class="st-note">Needs a chat model. Add one under <button class="link" onclick={() => app.go('models')}>Models</button>.</p>{/if}
			{#if studio.busy === 'writing'}<p class="st-note" role="status">The chat model is writing. This can take a minute or two.</p>{/if}
		</section>
		{@render narrationTool('the track named Narration')}
		<section class="st-sec">
			<h3 class="st-h">The exported file</h3>
			<div class="st-row">
				<span class="st-lab">Size</span>
				<span class="st-seg" role="radiogroup" aria-label="Size of the exported film">
					<button role="radio" aria-checked={doc.export.scale !== 2} onclick={() => studio.edit('Export size', (d) => (d.export.scale = 0))}>{filmWidth} × {filmHeight}</button>
					<button role="radio" aria-checked={doc.export.scale === 2} onclick={() => studio.edit('Export size', (d) => (d.export.scale = 2))} title="Enlarges the picture. It adds no detail, and the file is about two and a half times the size.">{filmWidth * 2} × {filmHeight * 2}</button>
				</span>
			</div>
			<div class="st-row">
				<span class="st-lab">Quality</span>
				<span class="st-seg" role="radiogroup" aria-label="Quality of the exported film">
					{#each [['small', 'Smaller'], ['', 'Balanced'], ['best', 'Best']] as [value, label] (value)}
						<button role="radio" aria-checked={(doc.export.quality ?? '') === value} onclick={() => studio.edit('Export quality', (d) => (d.export.quality = value))}>{label}</button>
					{/each}
				</span>
			</div>
			<div class="st-row">
				<span class="st-lab">File name</span>
				<input class="st-in grow" value={doc.export.name ?? ''} placeholder={doc.name} aria-label="Name of the exported file" oninput={(e) => studio.edit('Export name', (d) => (d.export.name = e.currentTarget.value), 'export-name')} onkeydown={(e) => e.stopPropagation()} />
				<span class="st-val left">.mp4</span>
			</div>
			<label class="st-row st-check" title="When the last shot is made, the film is joined into one MP4 file in its folder, also with this screen closed.">
				<input type="checkbox" checked={doc.auto_export} onchange={(e) => studio.edit('Export by itself', (d) => (d.auto_export = e.currentTarget.checked))} />
				Export by itself when the last shot is made
			</label>
			{#if studio.view?.export?.state === 'done' && studio.view.export.file}
				<p class="st-note selectable">Last export: {fileName(studio.view.export.file)}{studio.view.export.bytes ? `, ${bytesOf(studio.view.export.bytes)}` : ''}</p>
			{/if}
		</section>
	{/if}
</div>

<style>
	.inspector {
		flex: 1 1 0;
		min-height: 0;
		background: var(--st-panel);
	}

	.pad {
		padding: 10px;
	}

	.top {
		position: sticky;
		top: 0;
		z-index: 2;
		display: flex;
		align-items: center;
		gap: 7px;
		height: 28px;
		padding: 0 8px;
		background: var(--st-raised);
		border-bottom: 1px solid var(--st-edge);
	}

	.kind {
		font-size: 11.5px;
		font-weight: 700;
		color: var(--st-ink);
		white-space: nowrap;
	}

	.state {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 10.5px;
		color: var(--st-muted);
	}

	.state.done {
		color: var(--st-ok);
	}

	.state.failed {
		color: var(--st-fail);
	}

	.state.making,
	.state.queued {
		color: var(--st-accent);
	}

	.state.changed {
		color: var(--st-warn);
	}

	.grow {
		flex: 1;
		min-width: 0;
	}

	.chip {
		flex: none;
		width: 10px;
		height: 10px;
		border-radius: 2px;
	}

	.count {
		font-weight: 500;
		color: var(--st-faint);
	}

	.path {
		font: 500 11px/1.35 var(--rb-data);
		color: var(--st-ink);
		overflow-wrap: anywhere;
	}

	.path.one {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		color: var(--st-muted);
	}

	.warn {
		color: var(--st-warn);
	}

	.facts {
		display: grid;
		grid-template-columns: 72px minmax(0, 1fr);
		gap: 3px 6px;
		margin: 0;
		font-size: 11px;
	}

	.facts dt {
		color: var(--st-faint);
	}

	.facts dd {
		margin: 0;
		color: var(--st-muted);
		font-variant-numeric: tabular-nums;
	}

	.takes {
		display: grid;
		grid-template-columns: repeat(4, minmax(0, 1fr));
		gap: 4px;
	}

	.take {
		position: relative;
		aspect-ratio: var(--shape, 5 / 3);
		border-radius: 2px;
		background: #000 center / cover no-repeat;
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.1);
		opacity: 0.62;
	}

	.take:hover {
		opacity: 1;
	}

	.take span {
		position: absolute;
		left: 2px;
		bottom: 2px;
		min-width: 13px;
		padding: 0 3px;
		border-radius: 2px;
		background: rgba(0, 0, 0, 0.7);
		font: 600 9.5px/13px var(--rb-data);
		color: #fff;
	}

	.take[aria-pressed='true'] {
		opacity: 1;
		box-shadow:
			inset 0 0 0 1.5px #fff,
			inset 0 0 0 2.5px #000;
	}

	.swatches {
		display: inline-flex;
		gap: 4px;
	}

	.swatches button {
		width: 16px;
		height: 16px;
		border-radius: 3px;
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.4);
	}

	.swatches button[aria-checked='true'] {
		box-shadow:
			0 0 0 1.5px var(--st-panel),
			0 0 0 3px #fff;
	}

	.shape {
		display: block;
		height: 10px;
		max-width: 16px;
		border: 1.5px solid currentColor;
		border-radius: 1.5px;
	}
</style>
