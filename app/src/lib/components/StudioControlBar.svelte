<script lang="ts">
	import { onDestroy } from 'svelte';
	import { app } from '#lib/state.svelte.ts';
	import { dbLabel } from '#lib/studio-edit.ts';
	import { MASTER, studio } from '#lib/studio.svelte.ts';
	import { sliderToZoom, timecode, zoomToSlider } from '#lib/studio-time.ts';
	import { roughly } from '#lib/studio.ts';
	import { mod } from '#lib/words.ts';
	import StudioFader from './StudioFader.svelte';
	import StudioIcon from './StudioIcon.svelte';
	import StudioMeter from './StudioMeter.svelte';

	// The control bar: the transport, the display, what is being made, the panels, zoom, the
	// master volume, Make and Export.

	let big = $state<HTMLSpanElement | null>(null);
	const two = (n: number) => String(n).padStart(2, '0');

	/** The shot under the playhead, counted from one. */
	function shotAt(time: number) {
		const parts = studio.plan.parts;
		let at = -1;
		for (let i = 0; i < parts.length; i++) if (parts[i].start <= time + 1e-6) at = parts[i].index;
		return at + 1;
	}

	function show(time: number) {
		if (!big) return;
		big.textContent = studio.layout.lcd === 'time' ? timecode(time) : `${two(shotAt(time))} / ${two(studio.doc?.shots.length ?? 0)}`;
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
</script>

<div class="bar" role="toolbar" aria-label="Control bar">
	<div class="group">
		<button class="cb" class:on={studio.layout.library} aria-pressed={studio.layout.library} onclick={() => studio.set('library', !studio.layout.library)} title="Library (Y)" aria-label="Library"><StudioIcon name="library" /></button>
		<button class="cb" class:on={studio.layout.inspector && studio.layout.side !== 'assistant'} aria-pressed={studio.layout.inspector && studio.layout.side !== 'assistant'} onclick={() => studio.toggleInspector()} title="Inspector (I)" aria-label="Inspector"><StudioIcon name="inspector" /></button>
		<button class="cb" class:on={studio.layout.inspector && studio.layout.side === 'assistant'} aria-pressed={studio.layout.inspector && studio.layout.side === 'assistant'} onclick={() => studio.toggleAssistant()} title="Assistant (A)" aria-label="Assistant"><StudioIcon name="spark" /></button>
		<button class="cb" class:on={studio.layout.bottom && studio.layout.tab === 'mixer'} aria-pressed={studio.layout.bottom && studio.layout.tab === 'mixer'} onclick={() => studio.bottom('mixer')} title="Mixer (X)" aria-label="Mixer"><StudioIcon name="mixer" /></button>
		<button class="cb" class:on={studio.layout.bottom && studio.layout.tab === 'editor'} aria-pressed={studio.layout.bottom && studio.layout.tab === 'editor'} onclick={() => studio.bottom('editor')} title="Editor (E)" aria-label="Editor"><StudioIcon name="editor" /></button>
		<button class="cb" class:on={studio.layout.bottom && studio.layout.tab === 'cast'} aria-pressed={studio.layout.bottom && studio.layout.tab === 'cast'} onclick={() => studio.bottom('cast')} title="Cast (K)" aria-label="Cast"><StudioIcon name="cast" /></button>
		<span class="sep"></span>
		<button class="cb" onclick={() => studio.undo()} disabled={!canUndo} title={canUndo ? `Undo ${canUndo} (${mod}Z)` : 'Nothing to undo'} aria-label="Undo"><StudioIcon name="undo" /></button>
		<button class="cb" onclick={() => studio.redo()} disabled={!canRedo} title={canRedo ? `Redo ${canRedo} (Shift ${mod}Z)` : 'Nothing to redo'} aria-label="Redo"><StudioIcon name="redo" /></button>
	</div>

	<div class="mid">
		<div class="transport">
			<button class="tk" onclick={() => studio.seek(0)} title="Go to the start (Return)" aria-label="Go to the start"><StudioIcon name="start" size={15} /></button>
			<button class="tk play" class:lit={studio.playing} onclick={() => studio.togglePlay()} title="{studio.playing ? 'Pause' : 'Play'} (Space)" aria-label={studio.playing ? 'Pause' : 'Play'}><StudioIcon name={studio.playing ? 'pause' : 'play'} size={15} /></button>
			<button class="tk" onclick={() => studio.stop()} title="Stop. Again: back to the start." aria-label="Stop"><StudioIcon name="stop" size={15} /></button>
			<button class="tk cycle" class:lit={studio.cycle.on} aria-pressed={studio.cycle.on} onclick={() => studio.toggleCycle()} title="Cycle: play the range again and again (C). Drag in the top edge of the ruler to set the range." aria-label="Cycle"><StudioIcon name="cycle" size={15} /></button>
		</div>

		<div class="lcd">
			<button class="digits" onclick={() => studio.set('lcd', studio.layout.lcd === 'time' ? 'shot' : 'time')} title="Click to show {studio.layout.lcd === 'time' ? 'which shot the playhead is in' : 'the time'}" aria-label="Display: {studio.layout.lcd === 'time' ? 'hours, minutes, seconds, frames' : 'shot under the playhead, of how many'}">
				<span class="big" bind:this={big}>00:00:00:00</span>
				<span class="unit">{studio.layout.lcd === 'time' ? 'hr  min  sec  fr' : 'shot      of'}</span>
			</button>
			<div class="side">
				<span class="k">Length</span><span class="v">{timecode(studio.plan.total)}</span>
				<span class="k">Made</span><span class="v">{studio.shots.filter((s) => s.state === 'done').length} of {studio.doc?.shots.length ?? 0}</span>
			</div>
		</div>

		{#if progress || waiting > 0}
			<div class="status" role="status">
				<div class="line">
					<span class="dot"></span>
					<span class="txt">{makingAt ? `Making shot ${makingAt}` : 'Starting'}{progress?.step !== null && progress?.steps ? `, step ${progress.step} of ${progress.steps}` : ''}</span>
				</div>
				<div class="prog"><span style:width="{done * 100}%"></span></div>
				<span class="left">{waiting > 1 ? `${waiting} shots, ` : ''}{studio.view?.estimate && studio.view.estimate.left > 0 ? `${roughly(studio.view.estimate.left)} left` : 'working'}</span>
			</div>
			<button class="st-btn" onclick={() => studio.stopMaking()} title="Stop making. The shot being made is stopped; made shots stay.">Stop</button>
		{:else if exporting}
			<div class="status" role="status">
				<div class="line"><span class="dot"></span><span class="txt">Exporting {studio.view?.export?.shots ?? ''} shots</span></div>
				<div class="prog busy"><span></span></div>
				<span class="left">into one file</span>
			</div>
		{/if}
	</div>

	<div class="group right">
		<label class="zoom" title="Zoom ({mod}Left and {mod}Right, Z to fit)">
			<button class="cb small" onclick={() => studio.zoom(1 / 1.4)} aria-label="Zoom out">−</button>
			<input type="range" min="0" max="1" step="0.001" value={zoomToSlider(studio.pps)} oninput={(e) => (studio.pps = sliderToZoom(Number(e.currentTarget.value)))} aria-label="Zoom of the timeline" />
			<button class="cb small" onclick={() => studio.zoom(1.4)} aria-label="Zoom in">+</button>
		</label>
		<span class="sep"></span>
		{#if studio.doc}
			<div class="master" title="Master volume: {dbLabel(studio.doc.master)} dB">
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
		<span class="sep"></span>
		<button class="st-btn" onclick={() => studio.make()} disabled={studio.todo === 0 || !video || studio.view?.rendering} title={video ? `Make the shots that are not made (${mod}Return)` : 'Needs the video model'}>
			Make{studio.todo > 0 ? ` ${studio.todo}` : ''}
		</button>
		<button class="st-btn" onclick={() => studio.playFilm()} disabled={studio.exportPlan.parts.length === 0 && !studio.exportCurrent} title={studio.exportCurrent ? 'Play the exported film (P)' : studio.view?.film ? 'Play the film as it is now, large (P): it changed after its export' : 'Play the film, large (P)'}>
			<StudioIcon name="play" size={10} /> Play film
		</button>
		<button class="st-btn primary" onclick={() => studio.exportFilm()} disabled={studio.exportPlan.parts.length === 0 || exporting} title="Join the made shots and the sound tracks into one MP4 file in the film's folder ({mod}B)">{exporting ? 'Exporting…' : 'Export'}</button>
		<button class="cb" class:on={studio.keysOpen} onclick={() => (studio.keysOpen = !studio.keysOpen)} title="Keys (?)" aria-label="List of keys"><StudioIcon name="keys" /></button>
	</div>
</div>

<style>
	.bar {
		container-type: inline-size;
		flex: none;
		height: 50px;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 0 8px;
		background: linear-gradient(180deg, #26282d, #1d1f23);
		border-bottom: 1px solid var(--st-edge);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.05);
	}

	.group {
		flex: none;
		display: flex;
		align-items: center;
		gap: 3px;
	}

	.group.right {
		flex: 1 1 0;
		min-width: 0;
		justify-content: flex-end;
		gap: 5px;
	}

	.mid {
		flex: none;
		display: flex;
		align-items: center;
		gap: 10px;
		margin-left: 14px;
	}

	.sep {
		flex: none;
		width: 1px;
		height: 20px;
		margin: 0 4px;
		background: rgba(255, 255, 255, 0.07);
	}

	.cb {
		flex: none;
		width: 28px;
		height: 24px;
		display: grid;
		place-items: center;
		border-radius: 3.5px;
		color: var(--st-muted);
		background: rgba(255, 255, 255, 0.05);
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.35);
	}

	.cb.small {
		width: 18px;
		height: 18px;
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
		display: flex;
		gap: 1px;
		padding: 2px;
		border-radius: 5px;
		background: var(--st-edge);
	}

	.tk {
		width: 34px;
		height: 30px;
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
		height: 36px;
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
		width: 176px;
		padding: 0 12px;
		text-align: left;
	}

	.big {
		font: 500 20px/1 var(--rb-data);
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
		display: grid;
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

	.status {
		flex: none;
		overflow: hidden;
		width: 172px;
		display: grid;
		gap: 3px;
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
	}

	.zoom input {
		width: 76px;
		height: 12px;
		accent-color: var(--st-muted);
	}

	.master {
		display: flex;
		align-items: center;
		gap: 6px;
		width: 150px;
		color: var(--st-muted);
	}

	/* A narrow window keeps the transport, the display and the two buttons that matter. */
	@container (max-width: 1260px) {
		.zoom input {
			width: 52px;
		}
		.master {
			width: 124px;
		}
		/* While something is being made its line needs the room of the zoom. */
		.bar:has(.status) .zoom {
			display: none;
		}
	}

	@container (max-width: 1130px) {
		.side {
			display: none;
		}
	}

	@container (max-width: 980px) {
		.zoom,
		.master :global(.meter) {
			display: none;
		}
		.master {
			width: 84px;
		}
	}
</style>
