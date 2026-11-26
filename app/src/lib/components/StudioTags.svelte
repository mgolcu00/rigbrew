<script lang="ts">
	import { onDestroy, onMount, tick } from 'svelte';
	import { disabledAt, tagAt } from '#lib/studio-tags.ts';
	import type { Tag } from '#lib/studio-tools.ts';

	// The help tag: the name of the control under the pointer, one line of what it does, and its
	// key. It comes after a moment's rest on a control, like a pro app's help tags; once one has
	// shown, the next comes at once while the pointer goes along the controls. Keyboard focus
	// shows it too, and a control that cannot be used says why. Any press, key or scroll puts it
	// away.

	let { root }: { root: HTMLElement | null } = $props();

	const REST = 600;
	const FOCUS = 150;
	/** After a tag goes, the next one comes at once for this long. */
	const WARM = 900;

	let shown = $state<Tag | null>(null);
	let card = $state<HTMLDivElement | null>(null);
	let x = $state(-9999);
	let y = $state(-9999);
	let below = $state(true);
	let anchor: HTMLElement | null = null;
	let waiting: ReturnType<typeof setTimeout> | undefined;
	let warmUntil = 0;

	async function show(el: HTMLElement, tag: Tag) {
		anchor = el;
		shown = tag;
		x = -9999;
		await tick();
		if (!card || anchor !== el || !el.isConnected) return;
		const r = el.getBoundingClientRect();
		const w = card.offsetWidth;
		const h = card.offsetHeight;
		below = r.bottom + 6 + h <= innerHeight - 6;
		x = Math.round(Math.min(innerWidth - w - 6, Math.max(6, r.left + r.width / 2 - w / 2)));
		y = Math.round(below ? r.bottom + 6 : Math.max(6, r.top - h - 6));
	}

	function plan(el: HTMLElement, tag: Tag, wait: number) {
		clearTimeout(waiting);
		if (shown && anchor === el) return;
		if (shown) hide();
		anchor = el;
		if (wait <= 0) show(el, tag);
		else waiting = setTimeout(() => anchor === el && el.isConnected && show(el, tag), wait);
	}

	function hide() {
		clearTimeout(waiting);
		if (shown) warmUntil = performance.now() + WARM;
		shown = null;
		anchor = null;
	}

	function over(e: PointerEvent) {
		if (!root || e.pointerType === 'touch' || e.buttons) return;
		const found = tagAt(e.target, root) ?? disabledAt(e.target, e.clientX, e.clientY, root);
		if (!found) {
			if (anchor) hide();
			return;
		}
		if (found.el === anchor) return;
		plan(found.el, found.tag, performance.now() < warmUntil ? 0 : REST);
	}

	/** Moving inside one element can go from one disabled control to another: looked at once a frame. */
	let moved: PointerEvent | null = null;
	function move(e: PointerEvent) {
		if (!moved)
			requestAnimationFrame(() => {
				if (moved) over(moved);
				moved = null;
			});
		moved = e;
	}

	function out(e: PointerEvent) {
		// The pointer left the window.
		if (!e.relatedTarget) hide();
	}

	function focusIn(e: FocusEvent) {
		if (!root) return;
		const target = e.target as HTMLElement | null;
		if (!target?.matches?.(':focus-visible')) return;
		const found = tagAt(target, root);
		if (found) plan(found.el, found.tag, FOCUS);
	}

	function focusOut(e: FocusEvent) {
		if (anchor && e.target === anchor) hide();
	}

	function away() {
		if (anchor) {
			hide();
			warmUntil = 0;
		}
	}

	onMount(() => {
		addEventListener('pointerover', over, true);
		addEventListener('pointermove', move, { capture: true, passive: true });
		addEventListener('pointerout', out, true);
		addEventListener('pointerdown', away, true);
		addEventListener('keydown', away, true);
		addEventListener('wheel', away, { capture: true, passive: true });
		addEventListener('focusin', focusIn, true);
		addEventListener('focusout', focusOut, true);
	});

	onDestroy(() => {
		clearTimeout(waiting);
		removeEventListener('pointerover', over, true);
		removeEventListener('pointermove', move, true);
		removeEventListener('pointerout', out, true);
		removeEventListener('pointerdown', away, true);
		removeEventListener('keydown', away, true);
		removeEventListener('wheel', away, true);
		removeEventListener('focusin', focusIn, true);
		removeEventListener('focusout', focusOut, true);
	});
</script>

{#if shown}
	<div class="htag" class:above={!below} bind:this={card} role="tooltip" style:left="{x}px" style:top="{y}px">
		<div class="head">
			<span class="name">{shown.name}</span>
			{#if shown.key}<kbd>{shown.key}</kbd>{/if}
		</div>
		{#if shown.what}<p>{shown.what}</p>{/if}
	</div>
{/if}

<style>
	.htag {
		position: fixed;
		z-index: 80;
		max-width: 260px;
		padding: 6px 8px 7px;
		border-radius: 5px;
		background: #2b2d33;
		box-shadow:
			0 0 0 1px #000,
			inset 0 1px 0 rgba(255, 255, 255, 0.07),
			0 8px 24px rgba(0, 0, 0, 0.55);
		color: #eef0f3;
		pointer-events: none;
		animation: come 0.12s ease-out;
	}

	@keyframes come {
		from {
			opacity: 0;
			transform: translateY(-2px);
		}
	}

	.htag.above {
		animation-name: rise;
	}

	@keyframes rise {
		from {
			opacity: 0;
			transform: translateY(2px);
		}
	}

	.head {
		display: flex;
		align-items: center;
		gap: 10px;
	}

	.name {
		flex: 1;
		font: 600 11.5px/1.25 var(--rb-text);
		white-space: nowrap;
	}

	kbd {
		flex: none;
		padding: 1px 5px;
		border-radius: 3px;
		background: #17181b;
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.12);
		font: 500 10.5px/1.3 var(--rb-data);
		color: #cfd3da;
		white-space: nowrap;
	}

	p {
		margin: 3px 0 0;
		font: 400 11px/1.35 var(--rb-text);
		color: #a6abb5;
	}

	@media (prefers-reduced-motion: reduce) {
		.htag {
			animation: none;
		}
	}
</style>
