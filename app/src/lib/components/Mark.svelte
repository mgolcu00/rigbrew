<script lang="ts">
	// The Bay mark (docs/brand/mark-*.svg): an open frame, the machine, with a chamfered
	// module, the model, seated inside. Frame in ink, module in the accent; `mono` draws both
	// in the current text colour.
	let { size = 26, mono = false }: { size?: number; mono?: boolean } = $props();
</script>

<svg width={size} height={size} viewBox="0 0 64 64" fill="none" aria-hidden="true">
	<path d="M37 12H23A11 11 0 0 0 12 23v18a11 11 0 0 0 11 11h18a11 11 0 0 0 11-11V27" stroke={mono ? 'currentColor' : 'var(--ink)'} stroke-width="6" />
	<path d="M24 22h12l8 8v11a3 3 0 0 1-3 3H24a3 3 0 0 1-3-3V25a3 3 0 0 1 3-3z" fill={mono ? 'currentColor' : 'var(--accent)'} />
</svg>

<style>
	svg {
		flex: none;
		display: block;
	}
</style>
