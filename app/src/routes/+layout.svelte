<script lang="ts">
	import '../app.css';
	import martian from '@fontsource-variable/martian-mono/files/martian-mono-latin-wght-normal.woff2?url';
	import instrument from '@fontsource-variable/instrument-sans/files/instrument-sans-latin-wght-normal.woff2?url';

	let { children } = $props();

	// The window is an app, not a page: no browser menu on chrome. Text the person can select
	// or edit keeps its menu (copy, paste, look up).
	function oncontextmenu(event: MouseEvent) {
		const target = event.target as HTMLElement | null;
		if (target?.closest('.selectable, input, textarea, [contenteditable]')) return;
		event.preventDefault();
	}

	// Dropping a file on the window must not navigate away from the app.
	const ignore = (event: DragEvent) => event.preventDefault();
</script>

<svelte:head>
	<link rel="preload" href={martian} as="font" type="font/woff2" crossorigin="anonymous" />
	<link rel="preload" href={instrument} as="font" type="font/woff2" crossorigin="anonymous" />
</svelte:head>

<svelte:window {oncontextmenu} ondragover={ignore} ondrop={ignore} />

{@render children()}
