<script lang="ts">
	import { app } from '#lib/state.svelte.ts';
	import { STATE_COLOR, STATE_WORD } from '#lib/studio-look.ts';
	import { tag } from '#lib/studio-tags.ts';
	import { studio } from '#lib/studio.svelte.ts';
	import StudioIcon from './StudioIcon.svelte';

	// The cast of the film: who is in it and how each of them looks. A shot names its characters;
	// when it is made, the look of everyone it names goes into what the video model is told, word
	// for word, the same in every shot. A portrait shows who was written, and a shot can start
	// from it when the video model starts from pictures.

	const doc = $derived(studio.doc);
	const cast = $derived(doc?.cast ?? []);
	const member = $derived(cast.find((m) => m.id === studio.member) ?? cast[0]);
	const shots = $derived(member ? studio.shotsWith(member.id) : []);
	const picture = $derived(member ? studio.portrait(member.id) : undefined);
	const has = $derived(member ? studio.portraitCount(member.id) > 0 : false);
	const shape = $derived((doc?.size ?? '640x384').replace('x', ' / '));
	const starting = $derived(member ? studio.shots.filter((s) => s.from === member.id).length : 0);

	let words = $state('');

	async function write() {
		if (member && (await studio.writeLook(member.id, words))) words = '';
	}

	function remove() {
		if (!member) return;
		const id = member.id;
		const n = studio.shotsWith(id).length;
		if (n === 0) return studio.removeMember(id);
		studio.question = {
			text: `${member.name} is named in ${n} ${n === 1 ? 'shot' : 'shots'}. Without the character ${n === 1 ? 'it loses' : 'they lose'} the look, and made ones are marked Changed.`,
			yes: 'Remove',
			no: 'Keep',
			act: () => studio.removeMember(id)
		};
	}
</script>

<div class="cast" role="region" aria-label="Cast">
	<div class="list st-scroll">
		<ul>
			{#each cast as m (m.id)}
				{@const n = studio.shotsWith(m.id).length}
				<li>
					<button class="who" class:on={member?.id === m.id} aria-pressed={member?.id === m.id} onclick={() => (studio.member = m.id)}>
						<span class="face" style:background-image={studio.portrait(m.id) ? `url(${studio.portrait(m.id)})` : undefined}>{#if !studio.portrait(m.id)}<StudioIcon name="cast" size={11} />{/if}</span>
						<span class="name">{m.name || 'No name'}</span>
						<span class="meta" use:tag={{ name: `${n} ${n === 1 ? 'shot names' : 'shots name'} ${m.name || 'them'}`, what: 'A shot names a character in its description.' }}>{n}</span>
					</button>
				</li>
			{/each}
		</ul>
		<div class="tools">
			<button class="st-btn" onclick={() => studio.addMember()} disabled={cast.length >= 12} use:tag={{ id: 'add-character', what: cast.length >= 12 ? 'A film has at most twelve characters.' : 'A name and a look every shot naming them is made with.' }}><StudioIcon name="plus" size={11} /> Character</button>
		</div>
	</div>
	{#if member}
		<div class="detail st-scroll">
			<div class="portrait">
				<div class="frame" style:aspect-ratio={shape} style:background-image={picture ? `url(${picture})` : undefined}>
					{#if !picture}<span class="none">{studio.busy === 'portrait' ? 'Making…' : has ? '' : 'No portrait'}</span>{/if}
				</div>
				<button
					class="st-btn"
					onclick={() => studio.makePortrait(member.id)}
					disabled={!app.imagePackage || studio.busy !== '' || studio.view?.rendering || !member.look.trim()}
					use:tag={{ id: 'portrait', what: !app.imagePackage ? 'Needs the picture model: add it under Models.' : studio.view?.rendering ? 'Shots are being made; a portrait waits for them.' : !member.look.trim() ? 'Write the look first: the portrait is made from it.' : 'A picture made from the look, to start shots from.' }}
				>
					{studio.busy === 'portrait' ? 'Making…' : has ? 'Redo portrait' : 'Make portrait'}
				</button>
			</div>
			<div class="fields">
				<div class="st-row">
					<span class="st-lab">Name</span>
					<input
						class="st-in grow"
						value={member.name}
						maxlength="40"
						aria-label="Name of the character"
						oninput={(e) => studio.setMember(member.id, 'Name', (m) => (m.name = e.currentTarget.value), 'cast-name')}
						onkeydown={(e) => e.stopPropagation()}
					/>
					<span class="st-lab right">Voice</span>
					<select
						class="st-in"
						value={member.voice === null ? '' : String(member.voice)}
						aria-label="Voice that says {member.name}'s lines"
						disabled={!app.voicePackage}
						onchange={(e) => studio.setMember(member.id, 'Voice', (m) => (m.voice = e.currentTarget.value === '' ? null : Number(e.currentTarget.value)))}
					>
						<option value="">Film's voice</option>
						{#each { length: studio.view?.voices ?? studio.voices } as _, n (n)}<option value={String(n)}>Voice {n + 1}</option>{/each}
					</select>
				</div>
				<div class="st-row top">
					<span class="st-lab">Look</span>
					<textarea
						class="st-ta"
						rows="3"
						value={member.look}
						maxlength="400"
						placeholder="Age, build, face, hair, clothes, colours"
						aria-label="How {member.name} looks"
						oninput={(e) => studio.setMember(member.id, 'Look', (m) => (m.look = e.currentTarget.value), 'cast-look')}
						onkeydown={(e) => e.stopPropagation()}
					></textarea>
				</div>
				<div class="st-row">
					<span class="st-lab"></span>
					<input class="st-in grow" bind:value={words} placeholder="A few words: curious kid, yellow raincoat" aria-label="A few words about the character" onkeydown={(e) => (e.stopPropagation(), e.key === 'Enter' && write())} />
					<button class="st-btn" onclick={write} disabled={studio.busy !== '' || app.chatPackages.length === 0 || (!words.trim() && !member.look.trim() && !member.name.trim())} use:tag={{ id: 'write-look', what: app.chatPackages.length === 0 ? 'Needs the chat model: add it under Models.' : 'Writes the look from these words. Undo brings the old one back.' }}>
						<StudioIcon name="spark" size={12} />
						{studio.busy === 'looking' ? 'Writing…' : 'Write the look'}
					</button>
				</div>
				<div class="st-row">
					<span class="st-lab">In shots</span>
					{#if shots.length === 0}
						<span class="st-note">None. A shot names {member.name || 'the character'} in its description.</span>
					{:else}
						<span class="chips">
							{#each shots as s (s.id)}
								{@const at = studio.shots.findIndex((x) => x.id === s.id)}
								<button class="chip" class:on={studio.shotsSel.includes(s.id)} style:--c={STATE_COLOR[s.state]} onclick={() => (studio.selectShot(s.id), studio.seek(studio.plan.parts.find((p) => p.shot === s.id)?.start ?? 0))} use:tag={{ name: `Shot ${at + 1}: ${STATE_WORD[s.state]}`, what: 'Selects it and moves the playhead there.' }}>{at + 1}</button>
							{/each}
						</span>
					{/if}
				</div>
				<div class="st-row">
					<span class="st-lab"></span>
					<span class="st-note">The look goes into every shot that names {member.name || 'them'}, word for word.{starting > 0 ? ` ${starting} ${starting === 1 ? 'shot starts' : 'shots start'} from the portrait.` : ''}</span>
					<span class="grow"></span>
					<button class="st-btn quiet" onclick={remove} use:tag={{ name: 'Remove character', what: 'Takes them out of the cast; the shots keep their descriptions.' }}><StudioIcon name="trash" size={12} /> Remove</button>
				</div>
			</div>
		</div>
	{:else}
		<div class="detail empty">
			<p class="st-note">No characters. A character has a name and a look that every shot naming them is made with.</p>
		</div>
	{/if}
</div>

<style>
	.cast {
		height: 100%;
		min-height: 0;
		display: flex;
		background: var(--st-panel);
	}

	.list {
		flex: none;
		width: 196px;
		border-right: 1px solid var(--st-edge);
	}

	ul {
		margin: 0;
		padding: 0;
		list-style: none;
	}

	.who {
		width: 100%;
		height: 26px;
		display: flex;
		align-items: center;
		gap: 7px;
		padding: 0 8px 0 6px;
		text-align: left;
		color: var(--st-muted);
		border-bottom: 1px solid rgba(0, 0, 0, 0.4);
	}

	.who:hover {
		background: rgba(255, 255, 255, 0.04);
	}

	.who.on {
		background: rgba(91, 157, 240, 0.18);
		color: #fff;
	}

	.face {
		flex: none;
		width: 18px;
		height: 18px;
		display: grid;
		place-items: center;
		border-radius: 50%;
		background: var(--st-edge) center / cover;
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.08);
		color: var(--st-faint);
	}

	.name {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 11.5px;
		font-weight: 500;
	}

	.meta {
		flex: none;
		font: 500 10px/1 var(--rb-data);
		color: var(--st-faint);
	}

	.tools {
		padding: 7px 6px;
	}

	.detail {
		flex: 1;
		min-width: 0;
		display: flex;
		gap: 12px;
		padding: 9px 10px;
	}

	.detail.empty {
		align-items: flex-start;
	}

	.portrait {
		flex: none;
		width: 132px;
		display: grid;
		align-content: start;
		gap: 6px;
	}

	.frame {
		width: 100%;
		max-height: 150px;
		margin: 0 auto;
		display: grid;
		place-items: center;
		border-radius: 3px;
		background: #000 center / cover;
		box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.07);
	}

	.none {
		font-size: 10.5px;
		color: var(--st-faint);
	}

	.fields {
		flex: 1;
		min-width: 0;
		max-width: 620px;
		display: grid;
		align-content: start;
		gap: 6px;
	}

	.fields :global(.st-lab) {
		width: 52px;
	}

	.fields :global(.st-lab.right) {
		width: auto;
		margin-left: 6px;
	}

	.top {
		align-items: flex-start;
	}

	.top > :global(.st-lab) {
		padding-top: 5px;
	}

	.chips {
		display: flex;
		flex-wrap: wrap;
		gap: 3px;
	}

	.chip {
		min-width: 22px;
		height: 18px;
		padding: 0 5px;
		border-radius: 2.5px;
		background: var(--c);
		font: 600 10px/18px var(--rb-data);
		color: #fff;
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.35);
	}

	.chip.on {
		box-shadow:
			inset 0 0 0 1px rgba(0, 0, 0, 0.35),
			0 0 0 1.5px var(--st-playhead);
	}

	.grow {
		flex: 1;
	}
</style>
