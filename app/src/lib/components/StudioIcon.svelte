<script lang="ts" module>
	// The glyphs of the Studio workstation, on a 16-unit grid. Filled ones are transport keys;
	// the others are drawn with a 1.4 stroke.
	const filled = {
		play: 'M4.5 2.8v10.4L13 8z',
		pause: 'M4 3h3v10H4zM9 3h3v10H9z',
		stop: 'M3.5 3.5h9v9h-9z',
		start: 'M3 3h1.8v10H3zM13 3v10L5.6 8z',
		end: 'M11.2 3H13v10h-1.8zM3 3v10l7.4-5z',
		record: 'M8 3.2a4.8 4.8 0 1 0 0 9.6 4.8 4.8 0 0 0 0-9.6z'
	} as const;
	const drawn = {
		cycle: 'M3 6.5V5.8A1.8 1.8 0 0 1 4.8 4h6.4A1.8 1.8 0 0 1 13 5.8v4.4a1.8 1.8 0 0 1-1.8 1.8H6M7.8 9.8 5.6 12l2.2 2.2',
		library: 'M2.5 3.5h11v9h-11zM6 3.5v9',
		inspector: 'M2.5 3.5h11v9h-11zM10 3.5v9',
		bottom: 'M2.5 3.5h11v9h-11zM2.5 9h11',
		mixer: 'M4 2.5v11M8 2.5v11M12 2.5v11M2.7 5.5h2.6M6.7 10h2.6M10.7 7h2.6',
		editor: 'M2 8h1.5l1-3.5 1.6 7 1.5-9 1.6 8 1.3-4 1 1.5H14',
		viewer: 'M2.5 3.5h11v7.5h-11zM6 13.5h4',
		snap: 'M4 2.5v5a4 4 0 0 0 8 0v-5M4 5h2.2M9.8 5H12',
		plus: 'M8 3v10M3 8h10',
		close: 'M4 4l8 8M12 4l-8 8',
		undo: 'M5.5 3.5 2.8 6.2l2.7 2.7M3 6.2h6a3.6 3.6 0 0 1 0 7.2H6.5',
		redo: 'M10.5 3.5l2.7 2.7-2.7 2.7M13 6.2H7a3.6 3.6 0 0 0 0 7.2h2.5',
		link: 'M6.8 9.2a2.4 2.4 0 0 0 3.4 0l2-2a2.4 2.4 0 0 0-3.4-3.4l-.6.6M9.2 6.8a2.4 2.4 0 0 0-3.4 0l-2 2a2.4 2.4 0 0 0 3.4 3.4l.6-.6',
		trash: 'M3.5 4.8h9M6.4 4.8V3.5h3.2v1.3M4.8 4.8l.5 7.7h5.4l.5-7.7M6.9 7v3.5M9.1 7v3.5',
		copy: 'M6 6V4.3a.8.8 0 0 1 .8-.8h5a.8.8 0 0 1 .8.8v5a.8.8 0 0 1-.8.8H10M4.2 6h5a.8.8 0 0 1 .8.8v5a.8.8 0 0 1-.8.8h-5a.8.8 0 0 1-.8-.8v-5a.8.8 0 0 1 .8-.8z',
		keys: 'M2 4.5h12v7H2zM4.3 7v.1M6.8 7v.1M9.2 7v.1M11.7 7v.1M5 9.3h6',
		expand: 'M9.5 2.8h3.7v3.7M13 3 9.2 6.8M6.5 13.2H2.8V9.5M3 13l3.8-3.8',
		shrink: 'M13 6.5H9.5V3M9.7 6.3 13.2 2.8M3 9.5h3.5V13M6.3 9.7 2.8 13.2',
		speaker: 'M2.8 6.3h2.2L8 3.8v8.4L5 9.7H2.8zM10.3 6a2.8 2.8 0 0 1 0 4M11.9 4.3a5.2 5.2 0 0 1 0 7.4',
		film: 'M2.5 3.5h11v9h-11zM5 3.5v9M11 3.5v9M2.5 6.5H5M2.5 9.5H5M11 6.5h2.5M11 9.5h2.5',
		wave: 'M2.5 8v.1M4.7 6v4M6.9 3.5v9M9.1 5.5v5M11.3 4v8M13.5 7v2',
		chevron: 'M6 4l4 4-4 4',
		folder: 'M2.5 5A1 1 0 0 1 3.5 4h2.6l1.3 1.4h5.1a1 1 0 0 1 1 1v5.1a1 1 0 0 1-1 1h-9a1 1 0 0 1-1-1z',
		spark: 'M8 2.5v3M8 10.5v3M2.5 8h3M10.5 8h3M4.4 4.4l1.6 1.6M10 10l1.6 1.6M4.4 11.6 6 10M10 6l1.6-1.6',
		split: 'M8 2.5v11M5.5 5.5 3 8l2.5 2.5M10.5 5.5 13 8l-2.5 2.5',
		dup: 'M3 5.5h7v7H3zM6 3.5h7v7',
		back: 'M10 4 6 8l4 4'
	} as const;
	export type StudioIconName = keyof typeof filled | keyof typeof drawn;
</script>

<script lang="ts">
	let { name, size = 14 }: { name: StudioIconName; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 16 16" fill="none" aria-hidden="true">
	{#if name in filled}
		<path d={filled[name as keyof typeof filled]} fill="currentColor" />
	{:else}
		<path d={drawn[name as keyof typeof drawn]} stroke="currentColor" stroke-width="1.4" stroke-linecap="round" stroke-linejoin="round" />
	{/if}
</svg>

<style>
	svg {
		flex: none;
		display: block;
	}
</style>
