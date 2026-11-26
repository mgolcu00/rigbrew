<script lang="ts" generics="Id extends string">
	// The permission picker: one quiet menu in the box where you type. Code and Studio's
	// assistant use it with their own list of modes; the one that never asks is drawn in the
	// colour of a warning.

	let {
		modes,
		value,
		onchange,
		danger = '' as Id | '',
		label = 'Permissions'
	}: {
		modes: readonly { id: Id; name: string; does: string }[];
		value: Id;
		onchange: (mode: Id) => void;
		/** The mode that is shown as a warning. */
		danger?: Id | '';
		label?: string;
	} = $props();

	const mode = $derived(modes.find((m) => m.id === value) ?? modes[0]);
</script>

<label class="perm" class:danger={danger !== '' && value === danger} title={mode?.does}>
	<span class="sr">{label}</span>
	<select {value} onchange={(e) => onchange(e.currentTarget.value as Id)} aria-label={label}>
		{#each modes as m (m.id)}
			<option value={m.id}>{m.name}</option>
		{/each}
	</select>
</label>

<style>
	.perm {
		display: inline-flex;
		align-items: center;
	}

	.perm select {
		height: 28px;
		padding: 0 8px;
		border: 0;
		border-radius: 999px;
		background: var(--pressed);
		color: var(--ink);
		font: 500 13px/1 var(--rb-text);
		cursor: pointer;
	}

	.perm.danger select {
		background: var(--warn-soft);
		color: var(--warn);
	}
</style>
