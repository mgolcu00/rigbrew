<script lang="ts">
	import { tick } from 'svelte';
	import { TOUR_STEPS, guide } from '#lib/guide.svelte.ts';
	import { app } from '#lib/state.svelte.ts';
	import { machine } from '#lib/words.ts';

	// The first tour: four marks over the real window, each pointing at one thing and saying
	// what it is for. Esc or "Skip" ends it at any step; it is shown once, and Help replays it.

	interface Step {
		targets: string[];
		title: string;
		text: string;
	}

	const pictures = $derived(Boolean(app.imagePackage));

	const steps = $derived<Step[]>([
		{
			targets: ['[data-tour="composer"]'],
			title: 'Type here',
			text: 'Ask a question or say what you want, then press Enter. The answer appears above, and the chat is saved as you go.'
		},
		{
			targets: ['[data-tour="mode"]'],
			title: 'Chat or Picture',
			text: pictures
				? 'Chat answers in words. Picture makes an image from what you describe. Switch here at any time.'
				: 'Chat answers in words. Install Pictures in Models, and a Picture switch appears here to make images from a description.'
		},
		{
			targets: ['[data-tour="memory"]'],
			title: 'The memory line',
			text: `It shows which model is in your ${machine}’s memory and how much room is left. Only one model is loaded at a time; Rigbrew swaps them for you, which takes a few seconds.`
		},
		{
			targets: ['[data-tour="help"]', '[data-tour="profile"]'],
			title: 'Help, and your profile',
			text: 'Help, top right, explains everything in plain words. Your profile, bottom left, shows what you have done. Help can replay this tour.'
		}
	]);

	const step = $derived(guide.tour === null ? null : steps[Math.min(guide.tour, steps.length - 1)]);

	let rects = $state<DOMRect[]>([]);
	let vw = $state(0);
	let vh = $state(0);
	let next = $state<HTMLButtonElement>();
	let cardHeight = $state(160);

	function measure() {
		if (!step) return;
		vw = window.innerWidth;
		vh = window.innerHeight;
		rects = step.targets
			.map((s) => document.querySelector(s)?.getBoundingClientRect())
			.filter((r): r is DOMRect => Boolean(r && r.width > 0 && r.height > 0));
	}

	// The tour runs over the chat, with nothing on top of it.
	$effect(() => {
		if (guide.tour === null) return;
		if (app.screen !== 'chat') app.go('chat');
		app.sheet = null;
		app.palette = false;
	});

	$effect(() => {
		guide.tour;
		tick().then(() => {
			measure();
			next?.focus();
		});
		const timer = setInterval(measure, 400);
		return () => clearInterval(timer);
	});

	const pad = 4;
	const holes = $derived(rects.map((r) => ({ x: r.left - pad, y: r.top - pad, w: r.width + pad * 2, h: r.height + pad * 2 })));

	/** The card goes next to the first mark: above it when the mark is in the lower half. */
	const place = $derived.by(() => {
		const width = Math.min(340, vw - 24);
		const first = holes[0];
		if (!first) return { left: (vw - width) / 2, top: Math.max(24, vh / 2 - cardHeight / 2), width };
		const left = Math.max(12, Math.min(first.x + first.w - width, vw - width - 12, Math.max(12, first.x)));
		const below = first.y + first.h / 2 < vh / 2;
		const top = below ? first.y + first.h + 10 : first.y - cardHeight - 10;
		return { left, top: Math.max(12, Math.min(top, vh - cardHeight - 12)), width };
	});

	function onkeydown(event: KeyboardEvent) {
		if (guide.tour === null) return;
		if (event.key === 'Escape') {
			event.preventDefault();
			event.stopPropagation();
			guide.endTour(false);
		}
	}
</script>

<svelte:window onkeydowncapture={onkeydown} onresize={measure} />

{#if step && guide.tour !== null}
	<div class="tour">
		<svg class="veil" width={vw} height={vh} aria-hidden="true">
			<path
				fill-rule="evenodd"
				d="M0 0H{vw}V{vh}H0Z {holes.map((h) => `M${h.x} ${h.y}h${h.w}v${h.h}h${-h.w}Z`).join(' ')}"
			/>
			{#each holes as h, i (i)}
				<rect class="ring" x={h.x} y={h.y} width={h.w} height={h.h} rx="5" />
			{/each}
		</svg>

		<div
			class="card"
			bind:clientHeight={cardHeight}
			role="dialog"
			aria-modal="true"
			aria-labelledby="tour-title"
			aria-describedby="tour-text"
			style:left="{place.left}px"
			style:top="{place.top}px"
			style:width="{place.width}px"
		>
			<p class="count num">{guide.tour + 1} of {TOUR_STEPS}</p>
			<h2 id="tour-title">{step.title}</h2>
			<p id="tour-text" class="text">{step.text}</p>
			<p class="acts">
				<button class="btn quiet skip" onclick={() => guide.endTour(false)}>Skip tour</button>
				<span class="grow"></span>
				{#if guide.tour > 0}<button class="btn" onclick={() => guide.previousStep()}>Back</button>{/if}
				<button class="btn primary" bind:this={next} onclick={() => guide.nextStep()}>{guide.tour + 1 >= TOUR_STEPS ? 'Done' : 'Next'}</button>
			</p>
		</div>
	</div>
{/if}

<style>
	.tour {
		position: fixed;
		inset: 0;
		z-index: 80;
	}

	.veil {
		position: absolute;
		inset: 0;
	}

	.veil path {
		fill: color-mix(in srgb, var(--rb-board) 72%, transparent);
	}

	.ring {
		fill: none;
		stroke: var(--accent);
		stroke-width: 2;
	}

	.card {
		position: absolute;
		display: flex;
		flex-direction: column;
		gap: 4px;
		padding: 12px 14px;
		border: 1px solid var(--line);
		border-radius: 6px;
		background: var(--bg);
		box-shadow: 0 18px 50px -12px var(--shade);
		animation: appear 0.16s var(--ease) both;
	}

	.count {
		font-size: 11px;
		color: var(--muted);
	}

	h2 {
		font-size: 14px;
	}

	.text {
		font-size: 13px;
		line-height: 1.5;
	}

	.acts {
		display: flex;
		align-items: center;
		gap: 6px;
		margin-top: 8px;
	}

	.skip {
		margin-left: -8px;
	}

	.grow {
		flex: 1;
	}
</style>
