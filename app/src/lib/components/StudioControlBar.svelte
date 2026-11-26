<script lang="ts">
	import { onDestroy } from 'svelte';
	import { app } from '#lib/state.svelte.ts';
	import { dbLabel } from '#lib/studio-edit.ts';
	import { tag } from '#lib/studio-tags.ts';
	import { MASTER, studio } from '#lib/studio.svelte.ts';
	import { sliderToZoom, timecode, zoomToSlider } from '#lib/studio-time.ts';
	import { roughly } from '#lib/studio.ts';
	import StudioFader from './StudioFader.svelte';
	import StudioIcon from './StudioIcon.svelte';
	import StudioMeter from './StudioMeter.svelte';

	// The control bar, in the groups a pro app has: Edit (undo, redo), Transport, Time (the
	// display), Make (make the shots, play the film, export it), Mix (the master and the Mixer)
	// and View (the panels, zoom, the list of keys and tools). Each group is named under it; the
	// panel buttons say their names where the bar is wide enough, and are icons where it is not.

	let big = $state<HTMLSpanElement | null>(null);
	const two = (n: number) => String(n).padStart(2, '0');

	/** The shot under the playhead, counted from one. */
	function shotAt(time: number) {
		const parts = studio.plan.parts;
		let at = -1;
		for (let i = 0; i < parts.length; i++) if (parts[i].start <= time + 1e-6) at = parts[i].index;
		return at + 1;
	}

	/** The display is written when what it says changes (24 times a second while the film plays), not on every frame. */
	let said = '';
	function show(time: number) {
		if (!big) return;
		const text = studio.layout.lcd === 'time' ? timecode(time) : `${two(shotAt(time))} / ${two(studio.doc?.shots.length ?? 0)}`;
		if (text !== said || big.textContent !== text) big.textContent = said = text;
	}
	const stopFrames = studio.onFrame(show);
	$effect(() => {
		void studio.jumps;
		void studio.layout.lcd;
		void studio.plan;
		void big;
		show(studio.time);
	});
	onDestroy(stopFrames);

	const video = $derived(app.videoPackage);
	const progress = $derived(studio.view?.progress ?? null);
	const waiting = $derived(studio.shots.filter((s) => s.state === 'making' || s.state === 'queued').length);
	const exporting = $derived(studio.view?.export?.state === 'working');
	/** How far the shot being made is, 0 to 1. */
	const done = $derived(progress?.steps ? Math.min(1, (progress.step ?? 0) / progress.steps) : 0);
	const makingAt = $derived(progress ? studio.shots.findIndex((s) => s.id === progress.shot) + 1 : 0);
	const canUndo = $derived((studio.edits, studio.history.undoLabel));
	const canRedo = $derived((studio.edits, studio.history.redoLabel));
	const asking = $derived(studio.layout.inspector && studio.layout.side === 'assistant');
	const inspecting = $derived(studio.layout.inspector && studio.layout.side !== 'assistant');
	const below = (tab: 'mixer' | 'editor' | 'cast') => studio.layout.bottom && studio.layout.tab === tab;
	const makeWhat = $derived(!video ? 'Needs the video model: see the line under this bar.' : studio.todo === 0 ? 'Every shot with a description is made.' : `Makes the ${studio.todo === 1 ? 'shot' : `${studio.todo} shots`} not made yet.`);
</script>

<div class="bar" role="toolbar" aria-label="Control bar">
	<div class="grp" role="group" aria-label="Edit">
		<div class="row">
			<button class="cb" onclick={() => studio.undo()} disabled={!canUndo} use:tag={{ id: 'undo', name: canUndo ? `Undo ${canUndo}` : 'Undo', what: canUndo ? 'Takes back the last change.' : 'Nothing to undo yet.' }} aria-label="Undo"><StudioIcon name="undo" /></button>
			<button class="cb" onclick={() => studio.redo()} disabled={!canRedo} use:tag={{ id: 'redo', name: canRedo ? `Redo ${canRedo}` : 'Redo', what: canRedo ? 'Does the change again.' : 'Nothing to redo.' }} aria-label="Redo"><StudioIcon name="redo" /></button>
		</div>
		<span class="cap">Edit</span>
	</div>

	<div class="grp" role="group" aria-label="Transport">
		<div class="row transport">
			<button class="tk" onclick={() => studio.seek(0)} use:tag={'start'} aria-label="Go to the start"><StudioIcon name="start" size={14} /></button>
			<button class="tk play" class:lit={studio.playing} data-coach="play" onclick={() => studio.togglePlay()} use:tag={{ id: 'play', name: studio.playing ? 'Pause' : 'Play' }} aria-label={studio.playing ? 'Pause' : 'Play'}><StudioIcon name={studio.playing ? 'pause' : 'play'} size={14} /></button>
			<button class="tk" onclick={() => studio.stop()} use:tag={'stop'} aria-label="Stop"><StudioIcon name="stop" size={14} /></button>
			<button class="tk cycle" class:lit={studio.cycle.on} aria-pressed={studio.cycle.on} onclick={() => studio.toggleCycle()} use:tag={'cycle'} aria-label="Cycle"><StudioIcon name="cycle" size={14} /></button>
		</div>
		<span class="cap">Transport</span>
	</div>

	<div class="grp" role="group" aria-label="Time">
		<div class="lcd">
			<button class="digits" onclick={() => studio.set('lcd', studio.layout.lcd === 'time' ? 'shot' : 'time')} use:tag={{ id: 'display', what: studio.layout.lcd === 'time' ? 'Time at the playhead; click to show the shot instead.' : 'The shot under the playhead; click to show the time instead.' }} aria-label="Display: {studio.layout.lcd === 'time' ? 'hours, minutes, seconds, frames' : 'shot under the playhead, of how many'}">
				<span class="big" bind:this={big}>00:00:00:00</span>
				<span class="unit">{studio.layout.lcd === 'time' ? 'hr  min  sec  fr' : 'shot      of'}</span>
			</button>
			<div class="side" use:tag={'length'}>
				<span class="k">Length</span><span class="v">{timecode(studio.plan.total)}</span>
				<span class="k">Made</span><span class="v">{studio.shots.filter((s) => s.state === 'done').length} of {studio.doc?.shots.length ?? 0}</span>
			</div>
		</div>
		<span class="cap">Time</span>
	</div>

	<div class="grp" role="group" aria-label="Make">
		<div class="row">
			{#if progress || waiting > 0}
				<div class="status" role="status">
					<div class="line">
						<span class="dot"></span>
						<span class="txt">{makingAt ? `Making shot ${makingAt}` : 'Starting'}{progress?.step !== null && progress?.steps ? `, step ${progress.step} of ${progress.steps}` : ''}</span>
					</div>
					<div class="prog"><span style:width="{done * 100}%"></span></div>
					<span class="left">{waiting > 1 ? `${waiting} shots, ` : ''}{studio.view?.estimate && studio.view.estimate.left > 0 ? `${roughly(studio.view.estimate.left)} left` : 'working'}</span>
				</div>
				<button class="st-btn" onclick={() => studio.stopMaking()} use:tag={'stop-making'}>Stop</button>
			{:else if exporting}
				<div class="status" role="status">
					<div class="line"><span class="dot"></span><span class="txt">Exporting {studio.view?.export?.shots ?? ''} shots</span></div>
					<div class="prog busy"><span></span></div>
					<span class="left">into one file</span>
				</div>
			{/if}
			<button class="st-btn" data-coach="make" onclick={() => studio.make()} disabled={studio.todo === 0 || !video || studio.view?.rendering} use:tag={{ id: 'make', what: makeWhat }}>
				<StudioIcon name="clap" size={13} /> Make{studio.todo > 0 ? ` ${studio.todo}` : ''}
			</button>
			<button class="st-btn" onclick={() => studio.playFilm()} disabled={studio.exportPlan.parts.length === 0 && !studio.exportCurrent} use:tag={{ id: 'play-film', what: studio.exportCurrent ? 'Plays the exported film.' : studio.exportPlan.parts.length === 0 ? 'Make a shot first.' : 'Plays the whole film from its start, large.' }}>
				<StudioIcon name="filmplay" size={13} /> <span class="wide">Play film</span><span class="narrow">Film</span>
			</button>
			<button class="st-btn primary" onclick={() => studio.exportFilm()} disabled={studio.exportPlan.parts.length === 0 || exporting} use:tag={{ id: 'export', what: studio.exportPlan.parts.length === 0 ? 'Make a shot first: an export holds the made shots.' : 'Joins the made shots and the sound into one MP4 file.' }}>
				<StudioIcon name="export" size={13} />
				{exporting ? 'Exporting…' : 'Export'}
			</button>
		</div>
		<span class="cap">Make</span>
	</div>

	<span class="grow"></span>

	<div class="grp" role="group" aria-label="Mix">
		<div class="row">
			{#if studio.doc}
				<div class="master" use:tag={{ id: 'master', what: `How loud the whole film is: ${dbLabel(studio.doc.master)} dB.` }}>
					<StudioIcon name="speaker" size={13} />
					<StudioFader
						value={studio.doc.master}
						label="Master volume"
						onstart={() => studio.begin()}
						onchange={(v) => studio.live((d) => (d.master = v))}
						onend={() => studio.commit('Master volume')}
						onset={(v) => studio.edit('Master volume', (d) => (d.master = v), 'master')}
					/>
					<StudioMeter id={MASTER} length={54} thick={9} />
				</div>
			{/if}
			<button class="cb lab" class:on={below('mixer')} aria-pressed={below('mixer')} onclick={() => studio.bottom('mixer')} use:tag={'mixer'} aria-label="Mixer"><StudioIcon name="mixer" /><span>Mixer</span></button>
		</div>
		<span class="cap">Mix</span>
	</div>

	<div class="grp" role="group" aria-label="View">
		<div class="row">
			<button class="cb lab" class:on={studio.layout.library} aria-pressed={studio.layout.library} onclick={() => studio.set('library', !studio.layout.library)} use:tag={'library'} aria-label="Library"><StudioIcon name="library" /><span>Library</span></button>
			<button class="cb lab" class:on={inspecting} aria-pressed={inspecting} onclick={() => studio.toggleInspector()} use:tag={'inspector'} aria-label="Inspector"><StudioIcon name="inspector" /><span>Inspector</span></button>
			<button class="cb lab" class:on={asking} aria-pressed={asking} data-coach="assistant" onclick={() => studio.toggleAssistant()} use:tag={'assistant'} aria-label="Assistant"><StudioIcon name="ask" /><span>Assistant</span></button>
			<button class="cb lab" class:on={below('editor')} aria-pressed={below('editor')} onclick={() => studio.bottom('editor')} use:tag={'editor'} aria-label="Editor"><StudioIcon name="trim" /><span>Editor</span></button>
			<button class="cb lab" class:on={below('cast')} aria-pressed={below('cast')} onclick={() => studio.bottom('cast')} use:tag={'cast'} aria-label="Cast"><StudioIcon name="cast" /><span>Cast</span></button>
			<div class="zoom">
				<button class="cb small" onclick={() => studio.zoom(1 / 1.4)} use:tag={{ id: 'zoom', name: 'Zoom out' }} aria-label="Zoom out">−</button>
				<input type="range" min="0" max="1" step="0.001" value={zoomToSlider(studio.pps)} oninput={(e) => (studio.pps = sliderToZoom(Number(e.currentTarget.value)))} use:tag={'zoom'} aria-label="Zoom of the timeline" />
				<button class="cb small" onclick={() => studio.zoom(1.4)} use:tag={{ id: 'zoom', name: 'Zoom in' }} aria-label="Zoom in">+</button>
			</div>
			<button class="cb" class:on={studio.keysOpen} onclick={() => (studio.keysOpen = !studio.keysOpen)} use:tag={'keys'} aria-label="Keys and tools"><StudioIcon name="help" /></button>
		</div>
		<span class="cap">View</span>
	</div>
</div>

<style>
	.bar {
		container-type: inline-size;
		flex: none;
		height: 54px;
		display: flex;
		align-items: center;
		gap: 14px;
		padding: 0 10px;
		background: linear-gradient(180deg, #26282d, #1d1f23);
		border-bottom: 1px solid var(--st-edge);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
	}

	/* A group: its controls, and its name under them. */
	.grp {
		flex: none;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 3px;
		min-width: 0;
	}

	.row {
		display: flex;
		align-items: center;
		gap: 3px;
		height: 36px;
	}

	.cap {
		font: 600 8px/1 var(--rb-text);
		letter-spacing: 0.14em;
		text-transform: uppercase;
		color: #5c6370;
	}

	.grow {
		flex: 1 1 0;
		min-width: 0;
	}

	.cb {
		flex: none;
		height: 26px;
		min-width: 28px;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 5px;
		padding: 0 6px;
		border-radius: 3.5px;
		color: var(--st-muted);
		background: rgba(255, 255, 255, 0.05);
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.35);
		font: 500 11px/1 var(--rb-text);
		white-space: nowrap;
	}

	/* The name next to the icon, where the bar is wide enough. */
	.cb.lab span {
		display: none;
	}

	.cb.small {
		min-width: 18px;
		width: 18px;
		height: 18px;
		padding: 0;
		font: 500 13px/1 var(--rb-text);
		background: none;
		box-shadow: none;
	}

	.cb:hover:not(:disabled) {
		color: var(--st-ink);
		background: rgba(255, 255, 255, 0.11);
	}

	.cb:disabled {
		opacity: 0.35;
	}

	.cb.on {
		color: #10131a;
		background: var(--st-accent);
	}

	.transport {
		gap: 1px;
		height: auto;
		padding: 2px;
		border-radius: 5px;
		background: var(--st-edge);
	}

	.tk {
		width: 34px;
		height: 28px;
		display: grid;
		place-items: center;
		color: #c5c8cf;
		background: linear-gradient(180deg, #3a3d45, #2b2d33);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.09);
	}

	.tk:first-child {
		border-radius: 3.5px 0 0 3.5px;
	}

	.tk:last-child {
		border-radius: 0 3.5px 3.5px 0;
	}

	.tk:hover {
		color: #fff;
		background: linear-gradient(180deg, #454851, #33363d);
	}

	.tk:active {
		background: #25272c;
	}

	.tk.play.lit {
		color: #0c1a10;
		background: linear-gradient(180deg, #58d68a, #36b368);
	}

	.tk.cycle.lit {
		color: #201a08;
		background: linear-gradient(180deg, #f2cb58, #dba932);
	}

	/* The display: light figures on a dark glass. */
	.lcd {
		display: flex;
		align-items: stretch;
		height: 34px;
		border-radius: 5px;
		background: linear-gradient(180deg, #08090a, #101113);
		box-shadow:
			inset 0 0 0 1px #000,
			inset 0 2px 6px rgba(0, 0, 0, 0.8),
			0 1px 0 rgba(255, 255, 255, 0.06);
	}

	.digits {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		justify-content: center;
		gap: 2px;
		width: 168px;
		padding: 0 12px;
		text-align: left;
	}

	.big {
		font: 500 19px/1 var(--rb-data);
		font-variant-numeric: tabular-nums;
		letter-spacing: 0.02em;
		color: #dfe9f5;
		white-space: pre;
	}

	.unit {
		font: 500 8px/1 var(--rb-data);
		letter-spacing: 0.28em;
		text-transform: uppercase;
		color: #5c6370;
		white-space: pre;
	}

	.side {
		display: none;
		grid-template-columns: auto auto;
		align-content: center;
		gap: 4px 8px;
		padding: 0 12px;
		border-left: 1px solid rgba(255, 255, 255, 0.07);
	}

	.k {
		font: 500 8.5px/1 var(--rb-text);
		letter-spacing: 0.08em;
		text-transform: uppercase;
		color: #5c6370;
	}

	.v {
		font: 500 10.5px/1 var(--rb-data);
		font-variant-numeric: tabular-nums;
		color: #aeb8c6;
		white-space: nowrap;
	}

	.row :global(.st-btn) {
		height: 26px;
	}

	.narrow {
		display: none;
	}

	.status {
		flex: none;
		overflow: hidden;
		width: 160px;
		display: grid;
		gap: 3px;
		margin-right: 4px;
	}

	.line {
		display: flex;
		align-items: center;
		gap: 5px;
		min-width: 0;
	}

	.dot {
		flex: none;
		width: 6px;
		height: 6px;
		border-radius: 50%;
		background: var(--st-accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}

	.txt {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 10.5px;
		color: var(--st-ink);
	}

	.prog {
		height: 3px;
		border-radius: 2px;
		background: var(--st-edge);
		overflow: hidden;
	}

	.prog span {
		display: block;
		height: 100%;
		background: var(--st-accent);
		transition: width 0.4s linear;
	}

	.prog.busy span {
		width: 35%;
		animation: slide 1.1s ease-in-out infinite alternate;
	}

	@keyframes slide {
		from {
			transform: translateX(-20%);
		}
		to {
			transform: translateX(210%);
		}
	}

	.left {
		font-size: 10px;
		color: var(--st-faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.zoom {
		display: flex;
		align-items: center;
		gap: 1px;
		margin: 0 2px 0 6px;
	}

	.zoom input {
		width: 64px;
		height: 12px;
		accent-color: var(--st-muted);
	}

	.master {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 150px;
		margin-right: 4px;
		color: var(--st-muted);
	}

	/* How much the bar shows follows its width. Under 1440: icons, the master with its meter and
	   the zoom slider. From 1440 the panel buttons say their names, and make room for them: the
	   zoom keeps its buttons, the master its fader. From 1700 everything, with the film's length
	   and how many shots are made next to the display. */
	@container (min-width: 1440px) {
		.cb.lab span {
			display: inline;
		}
		.zoom input,
		.master :global(.meter) {
			display: none;
		}
		.master {
			width: 104px;
		}
	}

	@container (min-width: 1700px) {
		.side {
			display: grid;
		}
		.zoom input {
			display: block;
		}
		.master :global(.meter) {
			display: inline-flex;
		}
		.master {
			width: 150px;
		}
	}

	/* Narrower: the zoom slider and the master get smaller, the meter goes. */
	@container (max-width: 1220px) {
		.zoom input {
			width: 48px;
		}
		.master {
			width: 112px;
		}
		.master :global(.meter) {
			display: none;
		}
		.bar:has(.status) .zoom {
			display: none;
		}
	}

	/* Narrow: the master volume stays in the Mixer, and Play film is Film. */
	@container (max-width: 1150px) {
		.bar {
			gap: 10px;
		}
		.zoom input,
		.master {
			display: none;
		}
		.wide {
			display: none;
		}
		.narrow {
			display: inline;
		}
	}

	@container (max-width: 900px) {
		.cap,
		.zoom {
			display: none;
		}
	}
</style>
