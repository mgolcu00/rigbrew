<script lang="ts">
	import { onDestroy, onMount, untrack } from 'svelte';
	import { codeOf } from '#lib/errors.ts';
	import { bytes as bytesOf } from '#lib/format.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { app } from '#lib/state.svelte.ts';
	import { fileName } from '#lib/studio-look.ts';
	import { FILM, studio } from '#lib/studio.svelte.ts';
	import { shortTime } from '#lib/studio-time.ts';
	import { isMac, mod } from '#lib/words.ts';
	import StudioCast from '#lib/components/StudioCast.svelte';
	import StudioControlBar from '#lib/components/StudioControlBar.svelte';
	import StudioEditor from '#lib/components/StudioEditor.svelte';
	import StudioIcon from '#lib/components/StudioIcon.svelte';
	import StudioInspector from '#lib/components/StudioInspector.svelte';
	import StudioLibrary from '#lib/components/StudioLibrary.svelte';
	import StudioMixer from '#lib/components/StudioMixer.svelte';
	import StudioNeeds from '#lib/components/StudioNeeds.svelte';
	import StudioPlayer from '#lib/components/StudioPlayer.svelte';
	import StudioStart from '#lib/components/StudioStart.svelte';
	import StudioTracks from '#lib/components/StudioTracks.svelte';
	import StudioViewer from '#lib/components/StudioViewer.svelte';

	// Studio: a workstation for films made of shots. A control bar on top; the Library on the
	// left; the tracks in the middle with the Mixer, the Editor and the Cast under them; the viewer
	// and the Inspector on the right. Before a film is open, the start screen (StudioStart).

	const video = $derived(app.videoPackage);
	const tilde = (path: string) => path.replace(/^\/Users\/[^/]+/, '~');

	// --- the workstation ----------------------------------------------------------------------------

	let root = $state<HTMLElement | null>(null);
	let box = $state({ x: 0, y: 0, w: 0, h: 0 });
	const layout = $derived(studio.layout);
	const exported = $derived(studio.view?.export ?? null);
	/** What the service is doing to the film besides making shots, in a few words. */
	const working = $derived.by(() => {
		const count = (job: { done?: number; of?: number }) => (job.of ? ` ${Math.min(job.of, (job.done ?? 0) + 1)} of ${job.of}` : '');
		const { import: bring, dub, listen } = studio.jobs;
		if (bring?.state === 'working') return bring.stage === 'cutting' ? `Cutting the video into shots:${count(bring)}` : 'Reading the video…';
		if (dub?.state === 'working') return dub.stage === 'translating' ? `Translating into ${dub.language}…` : `Reading the lines aloud:${count(dub)}`;
		if (listen?.state === 'working') return `Listening to the clips:${count(listen)}`;
		return '';
	});
	const unmade = $derived(studio.plan.parts.length - studio.exportPlan.parts.length);

	// Pictures, waveforms and sound files are fetched as the film asks for them.
	$effect(() => studio.gather());
	// The mixer hears every change at once.
	$effect(() => studio.mix());
	// An export that has just been written opens the film player once.
	$effect(() => {
		void studio.view?.film?.modified_ms;
		void studio.view?.export?.state;
		untrack(() => studio.noticeExport());
	});
	// Regions that change while the film plays are set again from where the playhead is.
	$effect(() => {
		void JSON.stringify(studio.doc?.tracks.map((t) => [t.id, t.regions]));
		untrack(() => studio.replay());
	});

	/** Dragging a divider: a panel gets wider, narrower, taller or lower, and remembers it. */
	function resize(e: PointerEvent, key: 'libraryWidth' | 'inspectorWidth' | 'bottomHeight' | 'viewerHeight', sign: 1 | -1, min: number, max: number) {
		if (e.button !== 0) return;
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		const across = key === 'libraryWidth' || key === 'inspectorWidth';
		const from = across ? e.clientX : e.clientY;
		const was = layout[key];
		const onMove = (m: PointerEvent) => studio.set(key, Math.round(Math.min(max, Math.max(min, was + sign * ((across ? m.clientX : m.clientY) - from)))));
		const onUp = () => {
			el.removeEventListener('pointermove', onMove);
			el.removeEventListener('pointerup', onUp);
			el.removeEventListener('pointercancel', onUp);
		};
		el.addEventListener('pointermove', onMove);
		el.addEventListener('pointerup', onUp);
		el.addEventListener('pointercancel', onUp);
	}

	const KEYS: [string, string][][] = [
		[
			['Space', 'Play or pause'],
			['P', 'Play the film'],
			['Return', 'Go to the start'],
			[', and .', 'One frame back or on'],
			['Shift , and .', 'One second back or on'],
			['Home and End', 'Start or end of the film'],
			['Left and Right', 'The shot before or after'],
			['C', 'Cycle on or off'],
			[`${mod}Return`, 'Make the shots that are not made'],
			[`${mod}B`, 'Export the film']
		],
		[
			[`${mod}Z`, 'Undo'],
			[`Shift ${mod}Z`, 'Redo'],
			['Delete', 'Delete what is selected'],
			[`${mod}A`, 'Select everything on the track'],
			[`${mod}D`, 'Duplicate'],
			[`${mod}C and ${mod}V`, 'Copy and paste sound regions'],
			[`${mod}T`, 'Split at the playhead'],
			['M and S', 'Mute or solo the selected track'],
			['Alt, while dragging', 'Place freely, without snapping'],
			['Alt-drag a region', 'Copy it']
		],
		[
			[`${mod}Left and ${mod}Right`, 'Zoom out or in'],
			[`${mod}Up and ${mod}Down`, 'Tracks taller or lower'],
			['Z', 'Fit the film to the window'],
			['Pinch, or Ctrl and the wheel', 'Zoom around the pointer'],
			['Shift and the wheel', 'Scroll along the film'],
			['Y', 'Library'],
			['I', 'Inspector'],
			['X', 'Mixer'],
			['E', 'Editor'],
			['K', 'Cast'],
			['V', 'Viewer large or docked'],
			['?', 'This list']
		]
	];

	function keys(e: KeyboardEvent) {
		if (!studio.doc || e.defaultPrevented || app.palette || app.sheet || guide.helpOpen || studio.player) return;
		const t = e.target as HTMLElement | null;
		const typing = t instanceof HTMLTextAreaElement || t instanceof HTMLSelectElement || (t instanceof HTMLInputElement && !['range', 'checkbox', 'radio', 'button'].includes(t.type)) || Boolean(t?.isContentEditable);
		if (studio.question) {
			if (e.key === 'Enter' || e.key === 'Escape') {
				e.preventDefault();
				const q = studio.question;
				studio.question = null;
				if (e.key === 'Enter') q.act();
			}
			return;
		}
		if (typing) return;
		const cmd = e.metaKey || e.ctrlKey;
		const key = e.key.length === 1 ? e.key.toLowerCase() : e.key;
		let used = true;
		if (cmd && !e.altKey) {
			if (key === 'z') e.shiftKey ? studio.redo() : studio.undo();
			else if (key === 'y') studio.redo();
			else if (key === 'a') studio.selectAll();
			else if (key === 'd') studio.duplicateSelection();
			else if (key === 'c') studio.copy();
			else if (key === 'v') studio.paste();
			else if (key === 't') studio.splitAtPlayhead();
			else if (key === 'b') studio.exportFilm();
			else if (key === 'Enter') studio.make();
			else if (key === 'ArrowLeft') studio.zoom(1 / 1.4);
			else if (key === 'ArrowRight') studio.zoom(1.4);
			else if (key === 'ArrowUp') studio.set('vzoom', Math.min(2, layout.vzoom + 0.1));
			else if (key === 'ArrowDown') studio.set('vzoom', Math.max(0.6, layout.vzoom - 0.1));
			else used = false;
		} else if (e.altKey) used = false;
		else if (key === ' ') studio.togglePlay();
		else if (key === 'Enter') studio.seek(0);
		else if (e.code === 'Comma') studio.step(e.shiftKey ? -24 : -1);
		else if (e.code === 'Period') studio.step(e.shiftKey ? 24 : 1);
		else if (key === 'Home') studio.seek(0);
		else if (key === 'End') studio.seek(studio.end);
		else if (key === 'ArrowLeft' || key === 'ArrowRight') nextShot(key === 'ArrowRight' ? 1 : -1);
		else if (key === 'Delete' || key === 'Backspace') studio.deleteSelection();
		else if (key === '?') studio.keysOpen = !studio.keysOpen;
		else if (key === 'Escape') {
			if (studio.keysOpen) studio.keysOpen = false;
			else if (studio.viewerLarge) studio.viewerLarge = false;
			else if (studio.shotsSel.length + studio.regionsSel.length > 0 || studio.trackSel) studio.selectNothing();
			else used = false;
		} else if (e.shiftKey) used = false;
		else if (key === 'c') studio.toggleCycle();
		else if (key === 'm' && studio.track !== FILM) studio.toggle(studio.track, 'mute');
		else if (key === 's' && studio.track !== FILM) studio.toggle(studio.track, 'solo');
		else if (key === 'z') studio.fits++;
		else if (key === 'i') studio.set('inspector', !layout.inspector);
		else if (key === 'y') studio.set('library', !layout.library);
		else if (key === 'x') studio.bottom('mixer');
		else if (key === 'e') studio.bottom('editor');
		else if (key === 'k') studio.bottom('cast');
		else if (key === 'v') studio.viewerLarge = !studio.viewerLarge;
		else if (key === 'p') studio.playFilm();
		else used = false;
		if (used) {
			e.preventDefault();
			// A button that was clicked before does not take the key as its own.
			if (t instanceof HTMLButtonElement) t.blur();
		}
	}

	/** Left and Right: the shot before or after the selected one (or the one under the playhead), and the playhead to its start. */
	function nextShot(by: 1 | -1) {
		const parts = studio.plan.parts;
		if (parts.length === 0) return;
		let at = parts.findIndex((p) => p.shot === studio.shotsSel[0]);
		if (at < 0) {
			at = 0;
			for (let i = 0; i < parts.length; i++) if (parts[i].start <= studio.time + 1e-6) at = i;
			at -= by;
		}
		const next = parts[Math.min(parts.length - 1, Math.max(0, at + by))];
		studio.selectShot(next.shot);
		studio.seek(next.start + next.overlap);
	}

	let observer: ResizeObserver | undefined;
	onMount(() => {
		studio.refresh();
		if (studio.doc) studio.watch();
		if (root) {
			observer = new ResizeObserver(() => {
				const r = root!.getBoundingClientRect();
				box = { x: r.left, y: r.top, w: r.width, h: r.height };
			});
			observer.observe(root);
		}
	});
	onDestroy(() => {
		observer?.disconnect();
		studio.leave();
	});
</script>

<svelte:window onkeydown={keys} />

<section class="daw" bind:this={root} aria-label="Studio" style:--dx="{box.x}px" style:--dy="{box.y}px" style:--dw="{box.w}px" style:--dh="{box.h}px">
	{#if !studio.doc}
		<StudioStart />
	{:else}
		<StudioControlBar />
		{#if !studio.canMake}<StudioNeeds band chat={false} />{/if}
		{#if studio.question}
			{@const q = studio.question}
			<div class="ask" role="alertdialog" aria-label="A question before going on">
				<span class="selectable">{q.text}</span>
				<button class="st-btn primary" onclick={() => ((studio.question = null), q.act())}>{q.yes}</button>
				<button class="st-btn" onclick={() => (studio.question = null)}>{q.no}</button>
			</div>
		{/if}
		<div class="body">
			{#if layout.library}
				<aside class="left" style:width="{layout.libraryWidth}px">
					<StudioLibrary onnew={() => studio.close()} />
				</aside>
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div class="vdiv" onpointerdown={(e) => resize(e, 'libraryWidth', 1, 160, 420)}></div>
			{/if}
			<div class="centre">
				<StudioTracks />
				{#if layout.bottom}
					<!-- svelte-ignore a11y_no_static_element_interactions -->
					<div class="hdiv" onpointerdown={(e) => resize(e, 'bottomHeight', -1, 150, Math.max(160, box.h - 220))}></div>
					<div class="bottom" style:height="{layout.bottomHeight}px">
						<div class="st-tabs" role="tablist" aria-label="Bottom panel">
							<button role="tab" aria-selected={layout.tab === 'mixer'} onclick={() => studio.set('tab', 'mixer')}>Mixer</button>
							<button role="tab" aria-selected={layout.tab === 'editor'} onclick={() => studio.set('tab', 'editor')}>Editor</button>
							<button role="tab" aria-selected={layout.tab === 'cast'} onclick={() => studio.set('tab', 'cast')}>Cast</button>
							<span class="grow"></span>
							<button class="closer" onclick={() => studio.set('bottom', false)} aria-label="Close the panel" title="Close"><StudioIcon name="close" size={10} /></button>
						</div>
						<div class="panel">
							{#if layout.tab === 'mixer'}<StudioMixer />{:else if layout.tab === 'cast'}<StudioCast />{:else}<StudioEditor />{/if}
						</div>
					</div>
				{/if}
			</div>
			<!-- svelte-ignore a11y_no_static_element_interactions -->
			<div class="vdiv" class:gone={!layout.inspector} onpointerdown={(e) => resize(e, 'inspectorWidth', -1, 240, 460)}></div>
			<aside class="right" class:gone={!layout.inspector} style:width="{layout.inspector ? layout.inspectorWidth : 0}px">
				<div class="dock" style:height="{layout.viewerHeight}px">
					<StudioViewer />
				</div>
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div class="hdiv" onpointerdown={(e) => resize(e, 'viewerHeight', 1, 110, Math.max(120, box.h - 260))}></div>
				<StudioInspector />
			</aside>
		</div>
		<footer class="foot">
			{#if studio.problem}
				<span class="st-problem selectable" role="alert">{studio.problem}</span>
				<button class="closer" onclick={() => (studio.problem = '')} aria-label="Dismiss"><StudioIcon name="close" size={10} /></button>
			{:else if exported?.state === 'failed'}
				<span class="st-problem selectable" role="alert">The film could not be exported. Try again. {codeOf(exported.error ?? '')}</span>
			{:else if studio.silent.length > 0}
				<span class="st-problem selectable">{studio.silent.join(', ')} cannot be played in the app: {studio.silent.length === 1 ? 'it is' : 'they are'} silent here, and heard in the exported film.</span>
			{:else if working}
				<span class="note" role="status">{working}</span>
			{:else if studio.note}
				<span class="note">{studio.note}</span>
			{:else if unmade > 0 && studio.exportPlan.parts.length > 0}
				<span class="note">{unmade} {unmade === 1 ? 'shot is' : 'shots are'} not made. An export leaves {unmade === 1 ? 'it' : 'them'} out, and the sound after {unmade === 1 ? 'it' : 'them'} comes earlier against the pictures.</span>
			{:else if studio.doc.auto_export && (studio.view?.rendering || studio.todo > 0)}
				<span class="note">The film is exported by itself when the last shot is made.</span>
			{/if}
			<span class="grow"></span>
			{#if exported?.state === 'done' && exported.file}
				<span class="note selectable" title={tilde(exported.file)}>Saved: {fileName(exported.file)}{exported.seconds ? `, ${shortTime(exported.seconds)}` : ''}{exported.bytes ? `, ${bytesOf(exported.bytes)}` : ''}{exported.subtitles === 'burned' ? ', subtitles in the picture' : exported.subtitles === 'track' ? ', subtitle track' : exported.subtitles === 'file' ? ', subtitles as .srt' : ''}</span>
				<button class="flink" onclick={() => studio.reveal()}>{isMac ? 'Show in Finder' : 'Show in folder'}</button>
			{/if}
			{#if video?.credit}<span class="note credit" title={video.credit}>{video.credit}</span>{/if}
		</footer>
		{#if studio.keysOpen}
			<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
			<div class="veil" onclick={() => (studio.keysOpen = false)}>
				<div class="keys" role="dialog" aria-label="Keys" tabindex="-1" onclick={(e) => e.stopPropagation()}>
					<header>
						<h2>Keys</h2>
						<button class="closer" onclick={() => (studio.keysOpen = false)} aria-label="Close"><StudioIcon name="close" size={11} /></button>
					</header>
					<div class="cols">
						{#each KEYS as col, i (i)}
							<dl>
								{#each col as [k, what] (k)}
									<dt>{k}</dt>
									<dd>{what}</dd>
								{/each}
							</dl>
						{/each}
					</div>
				</div>
			</div>
		{/if}
		{#if studio.player}<StudioPlayer />{/if}
	{/if}
</section>

<style>
	/* The Studio is dark whatever the app is set to. Its own palette, and the app's variables
	   pointed at it, so shared classes (links, key caps, scroll bars) follow. */
	.daw {
		--st-bg: #0c0d0f;
		--st-panel: #151619;
		--st-raised: #1c1e22;
		--st-line: #2a2c32;
		--st-edge: #060607;
		--st-ink: #e3e5e9;
		--st-muted: #9a9ea8;
		--st-faint: #686c76;
		--st-accent: #5b9df0;
		--st-playhead: #f4f5f7;
		--st-cycle: #e3b341;
		--st-ok: #4fbf8b;
		--st-warn: #e0a93f;
		--st-fail: #f0766d;
		--bg: var(--st-panel);
		--ink: var(--st-ink);
		--muted: var(--st-muted);
		--faint: var(--st-faint);
		--line: var(--st-line);
		--line-soft: var(--st-line);
		--accent: var(--st-accent);
		--fail: var(--st-fail);
		--hover: rgba(255, 255, 255, 0.07);
		position: relative;
		height: 100%;
		min-height: 0;
		display: flex;
		flex-direction: column;
		background: var(--st-bg);
		color: var(--st-ink);
		font-size: 11.5px;
		line-height: 1.3;
		color-scheme: dark;
		user-select: none;
		-webkit-user-select: none;
	}

	/* The strip the window is dragged by, above the screen, is dark with it. It stays the app's:
	   this only paints over it, and the pointer goes through. */
	.daw::before {
		content: '';
		position: absolute;
		left: 0;
		right: 0;
		top: -28px;
		height: 28px;
		background: #1d1f23;
		pointer-events: none;
	}

	/* --- what every Studio panel is built from ---------------------------------------------- */

	.daw :global(.st-scroll) {
		overflow-y: auto;
		overflow-x: hidden;
		scrollbar-width: thin;
		scrollbar-color: #3a3d45 transparent;
		overscroll-behavior: contain;
	}

	.daw :global(.st-scroll-x) {
		overflow-x: auto;
		overflow-y: hidden;
		scrollbar-width: thin;
		scrollbar-color: #3a3d45 transparent;
	}

	.daw :global(.st-sec) {
		display: grid;
		gap: 6px;
		padding: 8px;
		border-bottom: 1px solid var(--st-edge);
		box-shadow: 0 1px 0 rgba(255, 255, 255, 0.025);
	}

	.daw :global(.st-h) {
		font: 600 10px/1 var(--rb-text);
		letter-spacing: 0.07em;
		text-transform: uppercase;
		color: var(--st-faint);
	}

	.daw :global(.st-row) {
		display: flex;
		align-items: center;
		gap: 6px;
		min-height: 20px;
		min-width: 0;
	}

	.daw :global(.st-lab) {
		flex: none;
		width: 64px;
		font-size: 11px;
		color: var(--st-muted);
	}

	.daw :global(.st-lab.right) {
		width: auto;
		margin-left: auto;
	}

	.daw :global(.st-val) {
		flex: none;
		min-width: 50px;
		font: 500 10.5px/1 var(--rb-data);
		font-variant-numeric: tabular-nums;
		text-align: right;
		color: var(--st-muted);
	}

	.daw :global(.st-val.left) {
		min-width: 0;
		text-align: left;
	}

	.daw :global(.st-note) {
		font-size: 11px;
		line-height: 1.4;
		color: var(--st-faint);
	}

	.daw :global(.st-problem) {
		padding: 0 8px;
		font-size: 11px;
		line-height: 1.4;
		color: var(--st-fail);
		overflow-wrap: anywhere;
	}

	.daw :global(.st-btn) {
		flex: none;
		display: inline-flex;
		align-items: center;
		justify-content: center;
		gap: 5px;
		height: 22px;
		padding: 0 9px;
		border-radius: 3.5px;
		background: linear-gradient(180deg, #3a3d45, #2e3037);
		box-shadow:
			inset 0 1px 0 rgba(255, 255, 255, 0.09),
			0 0 0 1px rgba(0, 0, 0, 0.55);
		font: 500 11px/1 var(--rb-text);
		color: var(--st-ink);
		white-space: nowrap;
	}

	.daw :global(.st-btn:hover:not(:disabled)) {
		background: linear-gradient(180deg, #454851, #363940);
	}

	.daw :global(.st-btn:active:not(:disabled)) {
		background: #292b31;
	}

	.daw :global(.st-btn:disabled) {
		opacity: 0.4;
	}

	.daw :global(.st-btn.primary) {
		background: linear-gradient(180deg, #6aa9f5, #4a8be0);
		color: #071224;
		font-weight: 600;
	}

	.daw :global(.st-btn.primary:hover:not(:disabled)) {
		background: linear-gradient(180deg, #7bb4f8, #5795e6);
	}

	.daw :global(.st-btn.quiet) {
		background: none;
		box-shadow: none;
		color: var(--st-muted);
	}

	.daw :global(.st-btn.quiet:hover:not(:disabled)) {
		background: rgba(255, 255, 255, 0.08);
		color: var(--st-ink);
	}

	.daw :global(.st-btn.icon) {
		width: 22px;
		padding: 0;
	}

	.daw :global(.st-btn.large) {
		height: 28px;
		padding: 0 14px;
		font-size: 12.5px;
	}

	.daw :global(.st-in),
	.daw :global(.st-ta) {
		min-width: 0;
		border: 0;
		border-radius: 3px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.07);
		color: var(--st-ink);
		font: 400 11.5px/1.4 var(--rb-text);
		outline: none;
		user-select: text;
		-webkit-user-select: text;
	}

	.daw :global(.st-in) {
		height: 22px;
		padding: 0 6px;
	}

	.daw :global(.st-in.grow) {
		flex: 1;
	}

	.daw :global(.st-in.num) {
		width: 104px;
		font-family: var(--rb-data);
		font-size: 10.5px;
	}

	.daw :global(.st-in.tiny) {
		width: 44px;
		font-family: var(--rb-data);
		text-align: right;
	}

	.daw :global(.st-in.wide) {
		width: 100%;
		height: 26px;
	}

	.daw :global(.st-ta) {
		width: 100%;
		padding: 6px 7px;
		resize: vertical;
	}

	.daw :global(.st-ta.big) {
		font-size: 12.5px;
		line-height: 1.45;
		min-height: 96px;
	}

	.daw :global(.st-in:focus),
	.daw :global(.st-ta:focus) {
		box-shadow: inset 0 0 0 1px var(--st-accent);
	}

	.daw :global(.st-in::placeholder),
	.daw :global(.st-ta::placeholder) {
		color: var(--st-faint);
	}

	.daw :global(.st-seg) {
		display: inline-flex;
		padding: 1px;
		border-radius: 4px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
	}

	.daw :global(.st-seg button) {
		min-width: 26px;
		height: 20px;
		padding: 0 8px;
		display: inline-grid;
		place-items: center;
		border-radius: 3px;
		font-size: 11px;
		color: var(--st-muted);
		white-space: nowrap;
	}

	.daw :global(.st-seg button:hover:not(:disabled)) {
		color: var(--st-ink);
	}

	.daw :global(.st-seg button[aria-checked='true']) {
		background: linear-gradient(180deg, #4a4d56, #3a3d45);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.1);
		color: #fff;
	}

	.daw :global(.st-check) {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-size: 11px;
		color: var(--st-muted);
		white-space: nowrap;
	}

	.daw :global(.st-check input) {
		width: 12px;
		height: 12px;
		margin: 0;
		accent-color: var(--st-accent);
	}

	.daw :global(.st-ms) {
		flex: none;
		width: 20px;
		height: 16px;
		border-radius: 2.5px;
		font: 700 9.5px/16px var(--rb-text);
		text-align: center;
		color: var(--st-muted);
		background: rgba(255, 255, 255, 0.06);
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.4);
	}

	.daw :global(.st-ms:hover) {
		color: var(--st-ink);
		background: rgba(255, 255, 255, 0.12);
	}

	.daw :global(.st-ms.m.lit) {
		color: #0c1620;
		background: #58b4f4;
	}

	.daw :global(.st-ms.s.lit) {
		color: #201a08;
		background: #f0c948;
	}

	.daw :global(.st-tabs) {
		flex: none;
		display: flex;
		align-items: stretch;
		height: 24px;
		background: var(--st-raised);
		border-bottom: 1px solid var(--st-edge);
	}

	.daw :global(.st-tabs [role='tab']) {
		padding: 0 11px;
		font-size: 11px;
		font-weight: 500;
		color: var(--st-muted);
		border-right: 1px solid rgba(0, 0, 0, 0.4);
	}

	.daw :global(.st-tabs [role='tab']:hover) {
		color: var(--st-ink);
	}

	.daw :global(.st-tabs [role='tab'][aria-selected='true']) {
		color: #fff;
		background: var(--st-panel);
		box-shadow: inset 0 2px 0 var(--st-accent);
	}

	.daw :global(button:focus-visible),
	.daw :global([role='slider']:focus-visible) {
		outline: 1.5px solid var(--st-accent);
		outline-offset: 1px;
	}

	/* --- the window ------------------------------------------------------------------------- */

	.body {
		flex: 1 1 0;
		min-height: 0;
		display: flex;
		overflow: hidden;
	}

	.left,
	.right {
		flex: none;
		min-width: 0;
		display: flex;
		flex-direction: column;
		background: var(--st-panel);
	}

	.right.gone {
		visibility: hidden;
		overflow: hidden;
	}

	.centre {
		flex: 1 1 0;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}

	.vdiv,
	.hdiv {
		flex: none;
		position: relative;
		z-index: 5;
		background: var(--st-edge);
	}

	.vdiv {
		width: 1px;
		cursor: col-resize;
	}

	.hdiv {
		height: 1px;
		cursor: row-resize;
	}

	.vdiv.gone {
		display: none;
	}

	/* A divider is one pixel to look at and seven to take hold of. */
	.vdiv::after,
	.hdiv::after {
		content: '';
		position: absolute;
		inset: 0 -3px;
	}

	.hdiv::after {
		inset: -3px 0;
	}

	.vdiv:hover,
	.hdiv:hover {
		background: var(--st-accent);
	}

	.bottom {
		flex: none;
		min-height: 0;
		display: flex;
		flex-direction: column;
		background: var(--st-panel);
	}

	.panel {
		flex: 1 1 0;
		min-height: 0;
	}

	.dock {
		flex: none;
		min-height: 0;
		background: #000;
	}

	.grow {
		flex: 1;
	}

	.closer {
		flex: none;
		width: 22px;
		display: grid;
		place-items: center;
		color: var(--st-faint);
	}

	.closer:hover {
		color: var(--st-ink);
	}

	.ask {
		flex: none;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 10px;
		background: #3a2f12;
		border-bottom: 1px solid var(--st-edge);
		font-size: 11.5px;
		color: #f3e3b3;
	}

	.ask span {
		flex: 1;
		min-width: 0;
	}

	.foot {
		flex: none;
		height: 22px;
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 0 8px;
		background: var(--st-raised);
		border-top: 1px solid var(--st-edge);
		white-space: nowrap;
		overflow: hidden;
	}

	.foot .note {
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		font-size: 10.5px;
		color: var(--st-muted);
	}

	.foot .credit {
		flex: 0 1 auto;
		max-width: 38%;
		color: var(--st-faint);
	}

	.foot :global(.st-problem) {
		min-width: 0;
		padding: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.flink {
		flex: none;
		font-size: 10.5px;
		color: var(--st-accent);
	}

	.flink:hover {
		text-decoration: underline;
	}

	.veil {
		position: absolute;
		inset: 0;
		z-index: 40;
		display: grid;
		place-items: center;
		background: rgba(0, 0, 0, 0.5);
	}

	.keys {
		width: min(860px, calc(100% - 40px));
		border-radius: 5px;
		background: var(--st-raised);
		box-shadow:
			0 0 0 1px #000,
			0 0 0 2px rgba(255, 255, 255, 0.06),
			0 24px 70px rgba(0, 0, 0, 0.7);
	}

	.keys header {
		display: flex;
		align-items: center;
		height: 30px;
		padding: 0 4px 0 12px;
		border-bottom: 1px solid var(--st-edge);
	}

	.keys h2 {
		flex: 1;
		font: 600 12px/1 var(--rb-text);
	}

	.keys .closer {
		height: 22px;
	}

	.cols {
		display: grid;
		grid-template-columns: repeat(3, minmax(0, 1fr));
		gap: 0 22px;
		padding: 10px 14px 14px;
	}

	.cols dl {
		display: grid;
		grid-template-columns: auto minmax(0, 1fr);
		align-content: start;
		gap: 5px 10px;
		margin: 0;
	}

	.cols dt {
		justify-self: start;
		align-self: start;
		padding: 2px 6px;
		border-radius: 3px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
		font: 500 10.5px/1.2 var(--rb-data);
		color: var(--st-ink);
		white-space: nowrap;
	}

	.cols dd {
		margin: 0;
		padding-top: 2px;
		font-size: 11px;
		color: var(--st-muted);
	}
</style>
