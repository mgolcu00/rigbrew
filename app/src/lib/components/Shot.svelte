<script lang="ts">
	import { agentShot } from '#lib/api.ts';

	// A picture a Code task took: a page it looked at, or a phone's screen. The service keeps it
	// behind the token, so it is fetched and shown through an object URL.
	let {
		task,
		name,
		alt,
		small = false
	}: {
		task: string;
		name: string;
		alt: string;
		/** A thumbnail in the transcript: a click makes it large and small again. */
		small?: boolean;
	} = $props();

	let src = $state('');
	let gone = $state(false);
	let large = $state(false);

	$effect(() => {
		let url = '';
		let live = true;
		src = '';
		gone = false;
		agentShot(task, name)
			.then((blob) => {
				if (!live) return;
				url = URL.createObjectURL(blob);
				src = url;
			})
			.catch(() => {
				if (live) gone = true;
			});
		return () => {
			live = false;
			if (url) URL.revokeObjectURL(url);
		};
	});
</script>

{#if src && small}
	<button class="shot" class:large onclick={() => (large = !large)} title={large ? 'Make it small' : 'Make it large'}>
		<img {src} {alt} />
	</button>
{:else if src}
	<img class="whole" {src} {alt} />
{:else if gone}
	<p class="gone">This picture is no longer kept.</p>
{/if}

<style>
	.shot {
		display: block;
		width: fit-content;
		max-width: 100%;
		margin: 4px 0 6px;
		border: 1px solid var(--line-soft);
		border-radius: 8px;
		overflow: hidden;
		cursor: zoom-in;
	}

	.shot img {
		display: block;
		max-width: 100%;
		max-height: 200px;
	}

	.shot.large {
		cursor: zoom-out;
	}

	.shot.large img {
		max-height: none;
	}

	.whole {
		display: block;
		max-width: 100%;
		max-height: 100%;
		margin: 0 auto;
		object-fit: contain;
	}

	.gone {
		font-size: 12px;
		color: var(--faint);
	}
</style>
