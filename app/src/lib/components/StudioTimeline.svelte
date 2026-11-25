<script lang="ts">
	import { tick } from 'svelte';
	import type { StudioShot } from '#lib/api.ts';
	import { hasClip, shotSeconds, stamp } from '#lib/studio.ts';

	// The film as a strip: one picture per shot, as wide as the shot is long. A shot that continues
	// the one before it sits against it with a small joint; a cut is a gap. Click selects, dragging
	// a picture moves the shot, Alt with the arrow keys moves the selected one.

	let {
		shots,
		posters,
		selected,
		playhead = null,
		onselect,
		onmove
	}: {
		shots: StudioShot[];
		/** Picture of each made shot, by shot id. */
		posters: Record<string, string>;
		selected: number;
		/** Where the film is playing: the shot and the seconds into its clip. */
		playhead?: { shot: string; at: number } | null;
		onselect: (index: number) => void;
		onmove: (from: number, to: number) => void;
	} = $props();

	const WIDTH = 46; // pixels per second, at most

	let strip = $state<HTMLElement | null>(null);
	let drag = $state<{ from: number; x0: number; dx: number; to: number; active: boolean; mark: number } | null>(null);
	let dragged = false;
	let head = $state<number | null>(null);

	const total = $derived(shots.reduce((n, s) => n + shotSeconds(s), 0));
	const thumbs = () => (strip ? Array.from(strip.querySelectorAll<HTMLElement>('[data-thumb]')) : []);
	const words: Record<StudioShot['state'], string> = { empty: 'not made', queued: 'waiting', making: 'being made', done: 'made', failed: 'did not work', changed: 'changed' };

	function down(e: PointerEvent, i: number) {
		if (e.button !== 0 || shots.length < 2) return;
		(e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
		drag = { from: i, x0: e.clientX, dx: 0, to: i, active: false, mark: 0 };
	}

	function over(e: PointerEvent) {
		if (!drag || !strip) return;
		const dx = e.clientX - drag.x0;
		const active = drag.active || Math.abs(dx) > 6;
		let to = drag.from;
		let mark = 0;
		if (active) {
			// The place it would take: after every other picture whose middle is left of the pointer.
			const others = thumbs().filter((_, i) => i !== drag!.from);
			const rects = others.map((t) => t.getBoundingClientRect());
			to = rects.filter((r) => r.left + r.width / 2 < e.clientX).length;
			const box = strip.getBoundingClientRect();
			const edge = to < rects.length ? rects[to].left - 4 : rects.length ? rects[rects.length - 1].right + 4 : box.left;
			mark = edge - box.left + strip.scrollLeft;
		}
		drag = { ...drag, dx, active, to, mark };
	}

	function up() {
		const d = drag;
		drag = null;
		if (!d?.active) return;
		dragged = true;
		setTimeout(() => (dragged = false), 0);
		if (d.to !== d.from) onmove(d.from, d.to);
	}

	function click(i: number) {
		if (!dragged) onselect(i);
	}

	async function key(e: KeyboardEvent, i: number) {
		const by = e.key === 'ArrowLeft' ? -1 : e.key === 'ArrowRight' ? 1 : 0;
		const to = i + by;
		if (!by || to < 0 || to >= shots.length) return;
		e.preventDefault();
		if (e.altKey) onmove(i, to);
		else onselect(to);
		await tick();
		thumbs()[to]?.focus();
	}

	// The playhead stands over the shot that is playing, as far in as the clip is.
	$effect(() => {
		const at = playhead;
		const index = at ? shots.findIndex((s) => s.id === at.shot) : -1;
		const thumb = index >= 0 ? thumbs()[index] : undefined;
		head = at && thumb ? thumb.offsetLeft + Math.min(1, at.at / shotSeconds(shots[index])) * thumb.offsetWidth : null;
	});
</script>

<div class="timeline">
	<div class="strip" bind:this={strip} role="group" aria-label="The shots in order. The arrow keys go from shot to shot; Alt with an arrow key moves the shot.">
		{#each shots as shot, i (shot.id || i)}
			{@const seconds = shotSeconds(shot)}
			<button
				type="button"
				data-thumb
				class="thumb {shot.state}"
				class:joined={i > 0 && shot.continues}
				class:on={i === selected}
				class:lifted={drag?.active && drag.from === i}
				aria-pressed={i === selected}
				aria-label="Shot {i + 1}, {Math.round(seconds * 10) / 10} seconds, {words[shot.state]}{i > 0 && shot.continues ? ', continues the shot before' : ''}"
				title={shot.prompt}
				style:flex-grow={seconds}
				style:max-width="{Math.max(26, seconds * WIDTH)}px"
				style:transform={drag?.active && drag.from === i ? `translateX(${drag.dx}px)` : undefined}
				onpointerdown={(e) => down(e, i)}
				onpointermove={over}
				onpointerup={up}
				onpointercancel={() => (drag = null)}
				onclick={() => click(i)}
				onkeydown={(e) => key(e, i)}
			>
				<span class="pic" style:background-image={hasClip(shot) && posters[shot.id] ? `url(${posters[shot.id]})` : undefined}>
					{#if hasClip(shot) && (shot.trim_start || shot.trim_end)}
						<span class="cut a" style:width="{((shot.trim_start ?? 0) / seconds) * 100}%"></span>
						<span class="cut b" style:width="{((shot.trim_end ?? 0) / seconds) * 100}%"></span>
					{/if}
				</span>
				<span class="num">{i + 1}</span>
			</button>
		{/each}
		{#if drag?.active && drag.to !== drag.from}<span class="mark" style:left="{drag.mark}px"></span>{/if}
		{#if head !== null}<span class="head" style:left="{head}px"></span>{/if}
	</div>
	<p class="scale">
		<span>{selected >= 0 && shots[selected] ? `Shot ${selected + 1} of ${shots.length}, ${Math.round(shotSeconds(shots[selected]) * 10) / 10} s` : 'Drag a picture to move its shot.'}</span>
		<span class="all">{stamp(total)}</span>
	</p>
</div>

<style>
	.timeline {
		display: grid;
		gap: 4px;
	}

	.strip {
		position: relative;
		display: flex;
		align-items: stretch;
		height: 62px;
		padding: 4px;
		border-radius: 10px;
		background: var(--well);
		overflow-x: auto;
		overflow-y: hidden;
		scrollbar-width: thin;
		touch-action: pan-y;
	}

	.thumb {
		position: relative;
		flex: 1 1 0;
		min-width: 26px;
		margin-left: 8px;
		border-radius: 6px;
		cursor: grab;
		transition: opacity var(--quick);
	}

	.thumb:first-child {
		margin-left: 0;
	}

	/* Continues the shot before it: no gap, and a joint on the seam. */
	.thumb.joined {
		margin-left: 1px;
	}

	.thumb.joined::before {
		content: '';
		position: absolute;
		z-index: 2;
		left: -5px;
		top: 50%;
		width: 9px;
		height: 9px;
		margin-top: -4.5px;
		border-radius: 50%;
		background: var(--bg);
		box-shadow: inset 0 0 0 2px var(--accent);
	}

	.pic {
		position: absolute;
		inset: 0;
		overflow: hidden;
		border-radius: 6px;
		background: color-mix(in srgb, var(--rb-ink) 9%, var(--bg)) center / cover no-repeat;
		box-shadow: inset 0 0 0 1px var(--line-soft);
	}

	.thumb.joined .pic {
		border-top-left-radius: 0;
		border-bottom-left-radius: 0;
	}

	.thumb.changed .pic,
	.thumb.failed .pic {
		opacity: 0.45;
	}

	.thumb.making .pic,
	.thumb.queued .pic {
		box-shadow: inset 0 0 0 1.5px var(--accent);
	}

	.thumb.on .pic {
		box-shadow:
			inset 0 0 0 2px var(--accent),
			inset 0 0 0 3px var(--bg);
	}

	.thumb:focus-visible {
		outline: 2px solid var(--accent);
		outline-offset: 1px;
	}

	.thumb.lifted {
		z-index: 3;
		opacity: 0.7;
		cursor: grabbing;
	}

	.num {
		position: absolute;
		left: 4px;
		bottom: 3px;
		min-width: 16px;
		padding: 0 4px;
		border-radius: 4px;
		background: color-mix(in srgb, var(--bg) 82%, transparent);
		font: 600 10.5px/16px var(--rb-data);
		color: var(--ink);
		text-align: center;
	}

	/* What the trims take away. */
	.cut {
		position: absolute;
		top: 0;
		bottom: 0;
		background: color-mix(in srgb, var(--rb-board) 72%, transparent);
	}

	.cut.a {
		left: 0;
	}

	.cut.b {
		right: 0;
	}

	.mark,
	.head {
		position: absolute;
		top: 2px;
		bottom: 2px;
		width: 2px;
		border-radius: 1px;
		pointer-events: none;
	}

	.mark {
		background: var(--accent);
	}

	.head {
		z-index: 4;
		margin-left: -1px;
		background: var(--ink);
		box-shadow: 0 0 0 1px var(--bg);
	}

	.scale {
		display: flex;
		justify-content: space-between;
		gap: 12px;
		padding: 0 4px;
		font-size: 12px;
		color: var(--faint);
	}

	.all {
		font-family: var(--rb-data);
	}
</style>
