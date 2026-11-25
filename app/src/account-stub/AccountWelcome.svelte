<script lang="ts">
	// The account step of the first launch, switched off: a build without accounts never shows it.
	let { ondone }: { ondone: () => void } = $props();
	$effect(() => ondone());
</script>
