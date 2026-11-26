<script lang="ts">
	import { tick } from 'svelte';
	import { studio } from '#lib/studio.svelte.ts';

	// The first time in the Studio: three marks over the control bar, one after the other. Each
	// points at a tool and says in a line what it is for. Skip, Esc or the last one's button puts
	// them away for good.

	const STEPS = [
		{ at: 'make', title: 'Make shots', text: 'Each shot is made from its description. Make starts every shot not made yet.' },
		{ at: 'play', title: 'Play', text: 'Plays the film from the playhead, here in the viewer. Space does the same.' },
		{ at: 'assistant', title: 'Ask the assistant', text: 'Say what to change in your own words. It edits the film while you watch, and one undo takes it back.' }
	];

	const step = $derived(studio.coach >= 0 && studio.coach < STEPS.length ? STEPS[studio.coach] : null);
	const hidden = $derived(!studio.doc || Boolean(studio.question) || studio.keysOpen || Boolean(studio.player) || studio.viewerLarge);

	let ring = $state<{ x: number; y: number; w: number; h: number } | null>(null);
	let card = $state<HTMLDivElement | null>(null);
	let next = $state<HTMLButtonElement | null>(null);
	let pos = $state({ x: 0, y: 0, arrow: 0 });

	function measure() {
		const el = step ? document.querySelector<HTMLElement>(`[data-coach="${step.at}"]`) : null;
		const r = el?.getBoundingClientRect();
		if (!r || r.width === 0) {
			ring = null;
			return;
		}
		ring = { x: r.left - 3, y: r.top - 3, w: r.width + 6, h: r.height + 6 };
		const w = card?.offsetWidth ?? 260;
		const middle = r.left + r.width / 2;
		const x = Math.round(Math.min(innerWidth - w - 10, Math.max(10, middle - w / 2)));
		pos = { x, y: Math.round(r.bottom + 12), arrow: Math.round(Math.min(w - 16, Math.max(16, middle - x))) };
	}

	$effect(() => {
		if (!step || hidden) return;
		tick().then(() => {
			measure();
			next?.focus({ preventScroll: true });
		});
		const timer = setInterval(measure, 400);
		addEventListener('resize', measure);
		return () => {
			clearInterval(timer);
			removeEventListener('resize', measure);
		};
	});

	function key(e: KeyboardEvent) {
		if (e.key !== 'Escape') return;
		e.stopPropagation();
		e.preventDefault();
		studio.endCoach();
	}
</script>

{#if step && !hidden}
	{#if ring}<div class="ring" style:left="{ring.x}px" style:top="{ring.y}px" style:width="{ring.w}px" style:height="{ring.h}px" aria-hidden="true"></div>{/if}
	<div class="coach" bind:this={card} role="dialog" aria-label="{step.title}: {studio.coach + 1} of {STEPS.length}" style:left="{pos.x}px" style:top="{pos.y}px" style:--arrow="{pos.arrow}px" tabindex="-1" onkeydown={key}>
		<div class="head">
			<strong>{step.title}</strong>
			<span class="count">{studio.coach + 1} of {STEPS.length}</span>
		</div>
		<p>{step.text}</p>
		<div class="foot">
			<button class="skip" onclick={() => studio.endCoach()}>Skip</button>
			<button class="st-btn primary" bind:this={next} onclick={() => (studio.coach + 1 < STEPS.length ? studio.coach++ : studio.endCoach())}>{studio.coach + 1 < STEPS.length ? 'Next' : 'Got it'}</button>
		</div>
	</div>
{/if}

<style>
	.ring {
		position: fixed;
		z-index: 70;
		border-radius: 6px;
		box-shadow:
			0 0 0 2px var(--st-accent),
			0 0 0 6px rgba(91, 157, 240, 0.25);
		pointer-events: none;
		animation: pulse 1.6s ease-in-out infinite;
	}

	@keyframes pulse {
		50% {
			box-shadow:
				0 0 0 2px var(--st-accent),
				0 0 0 9px rgba(91, 157, 240, 0.1);
		}
	}

	.coach {
		position: fixed;
		z-index: 71;
		width: 260px;
		padding: 10px 12px;
		border-radius: 6px;
		background: #2b2f38;
		box-shadow:
			0 0 0 1px #000,
			inset 0 1px 0 rgba(255, 255, 255, 0.08),
			0 14px 40px rgba(0, 0, 0, 0.6);
		color: var(--st-ink);
		outline: none;
	}

	/* The point toward the tool. */
	.coach::before {
		content: '';
		position: absolute;
		top: -6px;
		left: calc(var(--arrow) - 6px);
		width: 12px;
		height: 12px;
		background: #2b2f38;
		box-shadow: -1px -1px 0 #000;
		transform: rotate(45deg);
	}

	.head {
		display: flex;
		align-items: baseline;
		gap: 8px;
	}

	strong {
		flex: 1;
		font: 600 12.5px/1.3 var(--rb-text);
	}

	.count {
		font: 500 10px/1 var(--rb-data);
		color: var(--st-faint);
	}

	p {
		margin: 4px 0 10px;
		font-size: 11.5px;
		line-height: 1.4;
		color: #b9bec8;
	}

	.foot {
		display: flex;
		align-items: center;
		justify-content: flex-end;
		gap: 8px;
	}

	.skip {
		font-size: 11px;
		color: var(--st-muted);
	}

	.skip:hover {
		color: var(--st-ink);
	}

	@media (prefers-reduced-motion: reduce) {
		.ring {
			animation: none;
		}
	}
</style>
