<script lang="ts">
	import { FADER_MARKS, dbLabel, panLabel } from '#lib/studio-edit.ts';
	import { MASTER, SHOTS, studio } from '#lib/studio.svelte.ts';
	import StudioFader from './StudioFader.svelte';
	import StudioKnob from './StudioKnob.svelte';
	import StudioMeter from './StudioMeter.svelte';

	// The mixer: one channel strip for the shots' own sound, one for every sound track, and the
	// master. A strip is a pan knob, mute and solo, a tall fader beside its decibel scale and a
	// meter that holds its peak.

	let height = $state(0);
	/** The travel of the faders: what is left of the panel under the knobs and over the names. */
	const travel = $derived(Math.max(50, height - 154));

	const strip = (id: string) => (id === SHOTS ? studio.doc!.shot_track : studio.trackOf(id)!);
	const set = (id: string, what: 'volume' | 'pan', value: number) => {
		const s = strip(id);
		if (s) s[what] = value;
	};
	const marks = FADER_MARKS.filter(([, db]) => db !== -Infinity);
</script>

{#snippet scale()}
	<div class="scale" aria-hidden="true">
		{#each marks as [at, db] (db)}
			<span style:bottom="{at * 100}%">{db > 0 ? `+${db}` : db}</span>
		{/each}
	</div>
{/snippet}

<div class="mixer" bind:clientHeight={height} role="region" aria-label="Mixer">
	<div class="strips st-scroll-x">
		{#each studio.strips as s (s.id)}
			{@const track = s.id === SHOTS ? null : studio.trackOf(s.id)}
			{@const name = track?.name ?? 'Shot sound'}
			<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
			<div class="strip" class:on={studio.track === s.id} class:off={!studio.heard[s.id]} onclick={() => studio.selectTrack(s.id)}>
				<div class="pan">
					<StudioKnob
						value={s.pan}
						size={26}
						label="Pan of {name}"
						onstart={() => studio.begin()}
						onchange={(v) => studio.live(() => set(s.id, 'pan', v))}
						onend={() => studio.commit('Pan')}
						onset={(v) => studio.edit('Pan', () => set(s.id, 'pan', v), `pan:${s.id}`)}
					/>
					<span class="read">{panLabel(s.pan)}</span>
				</div>
				<div class="ms">
					<button class="st-ms m" class:lit={s.mute} aria-pressed={s.mute} onclick={(e) => (e.stopPropagation(), studio.toggle(s.id, 'mute'))} title="Mute" aria-label="Mute {name}">M</button>
					<button class="st-ms s" class:lit={s.solo} aria-pressed={s.solo} onclick={(e) => (e.stopPropagation(), studio.toggle(s.id, 'solo'))} title="Solo" aria-label="Solo {name}">S</button>
				</div>
				<div class="run" style:height="{travel}px">
					{@render scale()}
					<StudioFader
						vertical
						value={s.volume}
						label="Volume of {name}"
						onstart={() => studio.begin()}
						onchange={(v) => studio.live(() => set(s.id, 'volume', v))}
						onend={() => studio.commit('Volume')}
						onset={(v) => studio.edit('Volume', () => set(s.id, 'volume', v), `vol:${s.id}`)}
					/>
					<StudioMeter id={s.id} vertical length={travel} thick={9} />
				</div>
				<span class="db">{dbLabel(s.volume)}</span>
				<span class="name" style:--color={track?.color ?? '#1f4f5c'} title={name}>{name}</span>
			</div>
		{/each}
		<div class="gap"></div>
		{#if studio.doc}
			<div class="strip master">
				<div class="pan"><span class="read wide">Stereo out</span></div>
				<div class="ms"></div>
				<div class="run" style:height="{travel}px">
					{@render scale()}
					<StudioFader
						vertical
						value={studio.doc.master}
						label="Master volume"
						onstart={() => studio.begin()}
						onchange={(v) => studio.live((d) => (d.master = v))}
						onend={() => studio.commit('Master volume')}
						onset={(v) => studio.edit('Master volume', (d) => (d.master = v), 'master')}
					/>
					<StudioMeter id={MASTER} vertical length={travel} thick={13} />
				</div>
				<span class="db">{dbLabel(studio.doc.master)}</span>
				<span class="name" style:--color="#8b8f98">Master</span>
			</div>
		{/if}
	</div>
</div>

<style>
	.mixer {
		height: 100%;
		min-height: 0;
		background: var(--st-bg);
	}

	.strips {
		height: 100%;
		display: flex;
		align-items: stretch;
		padding: 0;
	}

	.strip {
		flex: none;
		width: 78px;
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 5px;
		padding: 7px 0 0;
		background: var(--st-raised);
		border-right: 1px solid var(--st-edge);
		box-shadow: inset 1px 0 0 rgba(255, 255, 255, 0.03);
	}

	.strip.on {
		background: #262930;
	}

	.strip.master {
		width: 88px;
		border-left: 1px solid var(--st-edge);
		background: #23252a;
	}

	.gap {
		flex: 1;
		min-width: 12px;
		background: var(--st-panel);
	}

	.pan {
		height: 40px;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: flex-end;
		gap: 2px;
	}

	.read {
		font: 500 9.5px/1 var(--rb-data);
		color: var(--st-faint);
	}

	.read.wide {
		padding-bottom: 6px;
		font-family: var(--rb-text);
		font-size: 10px;
	}

	.ms {
		height: 16px;
		display: flex;
		gap: 3px;
	}

	.run {
		display: flex;
		align-items: stretch;
		gap: 4px;
		padding: 15px 0;
		box-sizing: content-box;
	}

	/* The decibel marks beside the fader. */
	.scale {
		position: relative;
		width: 17px;
	}

	.scale span {
		position: absolute;
		right: 0;
		transform: translateY(50%);
		font: 500 8.5px/1 var(--rb-data);
		color: var(--st-faint);
		font-variant-numeric: tabular-nums;
	}

	.scale span::after {
		content: '';
		position: absolute;
		right: -4px;
		top: 50%;
		width: 3px;
		height: 1px;
		background: var(--st-line);
	}

	.db {
		width: 44px;
		height: 16px;
		border-radius: 2px;
		background: var(--st-edge);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
		font: 500 10px/16px var(--rb-data);
		font-variant-numeric: tabular-nums;
		text-align: center;
		color: var(--st-ok);
	}

	.off .db {
		color: var(--st-faint);
	}

	.name {
		align-self: stretch;
		margin-top: auto;
		height: 18px;
		padding: 0 4px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		background: color-mix(in srgb, var(--color) 72%, #000);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.14);
		font: 600 10.5px/18px var(--rb-text);
		text-align: center;
		color: #fff;
	}
</style>
