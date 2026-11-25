<script lang="ts" module>
	// One small icon set, drawn on a 20-unit grid with a 1.5 stroke.
	const shapes = {
		chat: 'M4 5.5A2.5 2.5 0 0 1 6.5 3h7A2.5 2.5 0 0 1 16 5.5v5a2.5 2.5 0 0 1-2.5 2.5H9.2l-3.7 3.2V13A2.5 2.5 0 0 1 4 10.6z',
		library: 'M3 16.5h14M4.5 16.5V9.2h3v7.3M8.5 16.5V5.5h3v11M12.5 16.5V7.6h3v8.9',
		rig: 'M7 5h6a2 2 0 0 1 2 2v6a2 2 0 0 1-2 2H7a2 2 0 0 1-2-2V7a2 2 0 0 1 2-2zM8.2 8.2h3.6v3.6H8.2zM8 2.5V5M12 2.5V5M8 15v2.5M12 15v2.5M2.5 8H5M2.5 12H5M15 8h2.5M15 12h2.5',
		privacy: 'M2.5 10S5.3 5 10 5s7.5 5 7.5 5-2.8 5-7.5 5-7.5-5-7.5-5zM10 7.8a2.2 2.2 0 1 0 0 4.4 2.2 2.2 0 0 0 0-4.4z',
		compose: 'M9.5 4H6a2 2 0 0 0-2 2v8a2 2 0 0 0 2 2h8a2 2 0 0 0 2-2v-3.5M14.4 3.4l2.2 2.2-6.4 6.4-2.7.5.5-2.7z',
		send: 'M10 15.5v-11M5.5 9 10 4.5 14.5 9',
		download: 'M10 3.5v9M6.2 9 10 12.8 13.8 9M4 16.5h12',
		check: 'M4.5 10.6 8 14l7.5-8.2',
		warn: 'M10 3.5 17.2 16H2.8zM10 8.6v3.4M10 13.9v.1',
		chevron: 'M7.5 5l5 5-5 5',
		external: 'M8.5 4.5h-3a1 1 0 0 0-1 1v9a1 1 0 0 0 1 1h9a1 1 0 0 0 1-1v-3M11.5 4.5h4v4M15.5 4.5l-6 6',
		memory: 'M3 6.5h14v7H3zM6 13.5v2.2M10 13.5v2.2M14 13.5v2.2M6.5 9v2M10 9v2M13.5 9v2',
		gpu: 'M3 6h12v8H3zM9 7.7a2.3 2.3 0 1 0 0 4.6 2.3 2.3 0 0 0 0-4.6zM15 8.2h2M15 11.8h2M5.5 14v2M12.5 14v2',
		disk: 'M4.5 4.5h11a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1zM3.5 11.5h13M13.6 13.5v.1',
		system: 'M3.5 4.5h13v9h-13zM7.5 16.5h5M10 13.5v3',
		// Families, by what they do.
		image: 'M4.5 4h11A1.5 1.5 0 0 1 17 5.5v9a1.5 1.5 0 0 1-1.5 1.5h-11A1.5 1.5 0 0 1 3 14.5v-9A1.5 1.5 0 0 1 4.5 4zM3.4 13.6l3.8-3.8 3.2 3.2 2-2 4.2 4.2M12.9 7.5v.1',
		video: 'M4.5 5h8A1.5 1.5 0 0 1 14 6.5v7a1.5 1.5 0 0 1-1.5 1.5h-8A1.5 1.5 0 0 1 3 13.5v-7A1.5 1.5 0 0 1 4.5 5zM14 8.8l3-1.8v6l-3-1.8',
		voice: 'M4 8.5v3M7 6v8M10 3.5v13M13 6v8M16 8.5v3',
		listening: 'M10 3a2.2 2.2 0 0 1 2.2 2.2v4.3a2.2 2.2 0 0 1-4.4 0V5.2A2.2 2.2 0 0 1 10 3zM5.5 9.5a4.5 4.5 0 0 0 9 0M10 14v3M7.5 17h5',
		copy: 'M7.5 7.5v-2a1 1 0 0 1 1-1h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-2M5.5 7.5h6a1 1 0 0 1 1 1v6a1 1 0 0 1-1 1h-6a1 1 0 0 1-1-1v-6a1 1 0 0 1 1-1z',
		redo: 'M15.5 10a5.5 5.5 0 1 1-1.7-4M15.8 3.2v3.2h-3.2',
		down: 'M10 4.5v11M5.5 11l4.5 4.5 4.5-4.5',
		settings: 'M3.5 6.5h5M12.5 6.5h4M3.5 13.5h4M11.5 13.5h5M10.5 4.5v4M9.5 11.5v4',
		person: 'M10 10.2a3.1 3.1 0 1 0 0-6.2 3.1 3.1 0 0 0 0 6.2zM4 16.5c.6-2.7 3-4.3 6-4.3s5.4 1.6 6 4.3',
		back: 'M12.5 5l-5 5 5 5',
		close: 'M5.5 5.5l9 9M14.5 5.5l-9 9',
		trash: 'M4.5 6h11M8 6V4.5h4V6M6 6l.6 9.5h6.8L14 6M8.6 8.8v4.4M11.4 8.8v4.4',
		eject: 'M10 4.5l5 6.5H5zM5 14.5h10',
		sync: 'M15.5 8.5a5.6 5.6 0 0 0-10-1.8M4.5 11.5a5.6 5.6 0 0 0 10 1.8M5.2 3.6v3.2h3.2M14.8 16.4v-3.2h-3.2',
		heart: 'M10 16s-6-3.6-6-8a3.3 3.3 0 0 1 6-1.9A3.3 3.3 0 0 1 16 8c0 4.4-6 8-6 8z',
		search: 'M9 4a5 5 0 1 0 0 10A5 5 0 0 0 9 4zM12.7 12.7l3.8 3.8',
		pane: 'M4.5 4.5h11a1 1 0 0 1 1 1v9a1 1 0 0 1-1 1h-11a1 1 0 0 1-1-1v-9a1 1 0 0 1 1-1zM12.5 4.5v11',
		plus: 'M10 4.5v11M4.5 10h11',
		return: 'M15.5 5v4.5a2 2 0 0 1-2 2H5M8 8.5l-3 3 3 3',
		gear: 'M10 7.6a2.4 2.4 0 1 0 0 4.8 2.4 2.4 0 0 0 0-4.8zM10 3v2M10 15v2M3 10h2M15 10h2M5.05 5.05l1.4 1.4M13.55 13.55l1.4 1.4M5.05 14.95l1.4-1.4M13.55 6.45l1.4-1.4',
		code: 'M7.2 6.5 3.5 10l3.7 3.5M12.8 6.5l3.7 3.5-3.7 3.5M11.4 4.5l-2.8 11',
		folder: 'M3 6.3A1.3 1.3 0 0 1 4.3 5h3.2l1.6 1.8h6.6A1.3 1.3 0 0 1 17 8.1v6.6a1.3 1.3 0 0 1-1.3 1.3H4.3A1.3 1.3 0 0 1 3 14.7z',
		bolt: 'M11 3 5 11h4.5L9 17l6-8h-4.5z'
	} as const;

	export type IconName = keyof typeof shapes | 'stop';
</script>

<script lang="ts">
	let { name, size = 18 }: { name: IconName; size?: number } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 20 20" fill="none" aria-hidden="true">
	{#if name === 'stop'}
		<rect x="5.5" y="5.5" width="9" height="9" rx="2" fill="currentColor" />
	{:else}
		<path d={shapes[name]} stroke="currentColor" stroke-width="1.5" stroke-linecap="round" stroke-linejoin="round" />
	{/if}
</svg>

<style>
	svg {
		flex: none;
		display: block;
	}
</style>
