<script lang="ts">
	import { onMount } from 'svelte';
	import { AccountPanel, getAccount } from '#account';
	import * as api from '#lib/api.ts';
	import { app } from '#lib/state.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { ago, bytes, clock, count, duration, seconds, size, variantName } from '#lib/format.ts';
	import type { ActivityDay, QueuedEvent, UsageTotals } from '#lib/types.ts';
	import { machine, nameById, versionLabel } from '#lib/words.ts';
	import Hint from '#lib/components/Hint.svelte';
	import Icon from '#lib/components/Icon.svelte';

	// The profile: who is using this install and what they have done with it. A year of
	// activity as a grid of days, totals, streaks, the models used most, the latest events,
	// and where the usage data stands. Signing in lives here.

	let now = $state(Date.now());
	let events = $state<QueuedEvent[]>([]);
	/** What happened lately, in the service's own words; null with an older service. */
	let recentItems = $state<Awaited<ReturnType<typeof api.recentActivity>>>(null);
	const flags = $derived(ux.flags);

	async function refresh() {
		app.refreshUsage();
		app.refreshActivity();
		try {
			recentItems = await api.recentActivity(50);
			// An older service: fall back to reading the events still waiting to be sent.
			if (recentItems === null) events = (await api.pendingEvents(200)).events;
		} catch {
			// keep the last list
		}
	}

	onMount(() => {
		refresh();
		const timer = setInterval(() => {
			now = Date.now();
			if (app.link === 'up') refresh();
		}, 6000);
		return () => clearInterval(timer);
	});

	const account = $derived(getAccount());
	const user = $derived(account.user);
	const initial = $derived(user && !user.anonymous ? (user.name ?? user.email ?? '?').trim().charAt(0).toUpperCase() : '');
	const activity = $derived(app.activity);
	const usage = $derived(app.usage);

	let copied = $state(false);
	async function copyId(text: string) {
		await api.copyText(text);
		copied = true;
		setTimeout(() => (copied = false), 1400);
	}

	// --- the grid ----------------------------------------------------------------------------------

	const DAY = 86_400_000;
	const months = ['Jan', 'Feb', 'Mar', 'Apr', 'May', 'Jun', 'Jul', 'Aug', 'Sep', 'Oct', 'Nov', 'Dec'];
	const weekdays = ['Sun', 'Mon', 'Tue', 'Wed', 'Thu', 'Fri', 'Sat'];
	/** 0 is Sunday. Day 0 of the epoch was a Thursday. */
	const weekday = (day: number) => (((day + 4) % 7) + 7) % 7;
	const dateOf = (day: number) => new Date(day * DAY);
	const dayName = (day: number) => {
		const d = dateOf(day);
		return `${weekdays[d.getUTCDay()]} ${d.getUTCDate()} ${months[d.getUTCMonth()]} ${d.getUTCFullYear()}`;
	};
	const weight = (d: ActivityDay | undefined) => (d ? d.prompts + d.images : 0);

	const today = $derived(activity?.today ?? Math.floor(now / DAY));
	const byDay = $derived(new Map((activity?.days ?? []).map((d) => [d.day, d])));
	const peak = $derived(Math.max(1, ...(activity?.days ?? []).map(weight)));

	interface Cell {
		day: number;
		data: ActivityDay | undefined;
		level: number;
		future: boolean;
	}

	// 53 columns of weeks, Sunday on top, the current week last.
	const weeks = $derived.by(() => {
		const first = today - weekday(today) - 52 * 7;
		const out: Cell[][] = [];
		for (let w = 0; w < 53; w++) {
			const column: Cell[] = [];
			for (let d = 0; d < 7; d++) {
				const day = first + w * 7 + d;
				const data = byDay.get(day);
				const value = weight(data);
				column.push({ day, data, future: day > today, level: value === 0 ? 0 : Math.min(4, 1 + Math.floor((value / peak) * 3.999 * 0.75)) });
			}
			out.push(column);
		}
		return out;
	});

	/** The month name over the first column that starts in it. */
	const monthMarks = $derived(
		weeks.map((column, i) => {
			const month = dateOf(column[0].day).getUTCMonth();
			const before = i > 0 ? dateOf(weeks[i - 1][0].day).getUTCMonth() : -1;
			return month !== before && i < 52 ? months[month] : '';
		})
	);

	let tip = $state<{ cell: Cell; x: number; y: number } | null>(null);
	let gridEl = $state<HTMLElement>();
	let calWidth = $state(0);
	/** The side of one day, so 53 weeks fill the column: between 8 and 15 pixels. */
	const cellSize = $derived(Math.max(8, Math.min(15, Math.floor((calWidth - 30) / 53) - 2)));

	function hover(event: MouseEvent, cell: Cell) {
		if (cell.future || !gridEl) return;
		const box = gridEl.getBoundingClientRect();
		const at = (event.currentTarget as HTMLElement).getBoundingClientRect();
		tip = { cell, x: at.left - box.left + at.width / 2, y: at.top - box.top };
	}

	// --- streaks -------------------------------------------------------------------------------------

	const streaks = $derived.by(() => {
		const active = (activity?.days ?? []).filter((d) => d.events > 0).map((d) => d.day).sort((a, b) => a - b);
		const set = new Set(active);
		let longest = 0;
		let run = 0;
		let previous = -10;
		for (const day of active) {
			run = day === previous + 1 ? run + 1 : 1;
			longest = Math.max(longest, run);
			previous = day;
		}
		// The current streak may end yesterday: today is not over yet.
		let current = 0;
		for (let day = set.has(today) ? today : today - 1; set.has(day); day--) current++;
		const busiest = (activity?.days ?? []).reduce<ActivityDay | null>((best, d) => (weight(d) > weight(best ?? undefined) ? d : best), null);
		return { current, longest, days: active.length, busiest };
	});

	const days = (n: number) => `${n} ${n === 1 ? 'day' : 'days'}`;

	const diskVersion = (id: string, variant: string) => {
		const pkg = app.packages.find((p) => p.id === id);
		return pkg ? versionLabel(pkg, variant, flags) : variantName(variant);
	};

	// --- totals ----------------------------------------------------------------------------------------

	const totals = $derived<{ label: string; value: (t: UsageTotals) => string; title?: string }[]>(
		flags.techDetails
			? [
					{ label: 'Messages', value: (t) => count(t.chat_messages) },
					{ label: 'Tokens', value: (t) => count(t.tokens_in + t.tokens_out) },
					{ label: 'Images', value: (t) => count(t.images) },
					{ label: 'Time generating', value: (t) => duration(t.seconds_generating) }
				]
			: [
					{ label: 'Messages', value: (t) => count(t.chat_messages) },
					{ label: 'Pictures', value: (t) => count(t.images) },
					{ label: 'Time working', value: (t) => duration(t.seconds_generating), title: 'How long models spent writing answers and making pictures' }
				]
	);

	const used = $derived(activity?.packages ?? []);
	const usedPeak = $derived(Math.max(1, ...used.map((p) => p.prompts)));

	// --- recent ----------------------------------------------------------------------------------------

	const nameOf = (id: unknown) => nameById(app.packages, typeof id === 'string' ? id : null, flags, 'a model');

	interface Line {
		id: string;
		at: number;
		what: string;
		detail: string;
	}

	/** One readable line per event worth showing; null for the rest. */
	function line(event: QueuedEvent): Line | null {
		const p = (event.payload ?? {}) as Record<string, any>;
		const base = { id: event.id, at: event.ts_ms };
		switch (event.kind) {
			case 'response':
				return { ...base, what: `${nameOf(p.package)} answered`, detail: [p.tokens_out ? `${p.tokens_out} tok` : '', p.tokens_per_second ? `${p.tokens_per_second.toFixed(1)} tok/s` : '', p.seconds ? seconds(p.seconds) : ''].filter(Boolean).join(' · ') };
			case 'image_generated':
				return { ...base, what: `${nameOf(p.package)} made an image`, detail: [p.width && p.height ? `${p.width}×${p.height}` : '', p.seconds ? seconds(p.seconds) : ''].filter(Boolean).join(' · ') };
			case 'image_failed':
				return { ...base, what: `${nameOf(p.package)} could not make an image`, detail: '' };
			case 'image_saved':
				return { ...base, what: 'Saved an image', detail: '' };
			case 'package_pull_finished':
				return p.error ? null : { ...base, what: `Installed ${nameOf(p.package)}`, detail: p.variant ? variantName(p.variant) : '' };
			case 'package_removed':
				return { ...base, what: `Removed ${nameOf(p.package)}`, detail: p.freed_bytes ? `freed ${bytes(p.freed_bytes)}` : '' };
			case 'engine_started':
				return { ...base, what: `Loaded ${nameOf(p.package)}`, detail: [p.variant ? variantName(p.variant) : '', p.load_seconds ? `in ${seconds(p.load_seconds)}` : ''].filter(Boolean).join(' · ') };
			case 'engine_unloaded':
				return { ...base, what: 'Unloaded the model', detail: '' };
			case 'chat_started':
				return { ...base, what: 'Started a chat', detail: p.mode === 'image' ? 'image mode' : '' };
			case 'consent_accepted':
				return { ...base, what: 'Accepted the terms', detail: '' };
			default:
				return null;
		}
	}

	const recent = $derived(
		recentItems
			? recentItems.map((item, i) => ({ id: `${item.ts_ms}-${i}`, at: item.ts_ms, what: item.title, detail: item.detail })).slice(0, 30)
			: events.map(line).filter((l): l is Line => l !== null).slice(0, 24)
	);
	const recentDays = $derived([...(activity?.days ?? [])].sort((a, b) => b.day - a.day).slice(0, 10));

	// --- sending ---------------------------------------------------------------------------------------

	let sending = $state(false);
	let syncProblem = $state('');

	async function sendNow() {
		sending = true;
		syncProblem = await app.sync();
		sending = false;
	}
</script>

<section class="screen" aria-label="Profile">
	<div class="body scroll">
		<div class="grid">
			<header class="id">
				<span class="avatar" class:named={initial !== ''}>{#if initial}{initial}{:else}<Icon name="person" size={24} />{/if}</span>
				<div class="who selectable">
					<h1>{user && !user.anonymous ? account.label : 'Your activity'}</h1>
					<p class="num meta">
						{#if user && flags.techDetails}
							<button class="uid" onclick={() => copyId(user.uid)} title="Copy the account id: {user.uid}">{copied ? 'copied' : user.uid.slice(0, 8)}</button>
							<span class="sep">{account.detail}</span>
						{:else if activity && flags.techDetails}
							<button class="uid" onclick={() => copyId(activity.install_id)} title="Copy the install id: {activity.install_id}">{copied ? 'copied' : activity.install_id.slice(0, 8)}</button>
							<span class="sep">{account.detail}</span>
						{:else if (!user || user.anonymous) && !flags.techDetails}
							<span class="plainmeta">Everything here is counted on this {machine}</span>
						{:else}
							{account.detail}
						{/if}
						{#if activity?.first_event_ms}<span class="sep">using Rigbrew since {new Date(activity.first_event_ms).toLocaleDateString([], { year: 'numeric', month: 'short', day: 'numeric' })}</span>{/if}
					</p>
				</div>
				<AccountPanel />
			</header>

			<section class="year" aria-label="Activity in the last year">
				<h2 class="rule label">
					{#if activity}{count(activity.days.reduce((n, d) => n + d.prompts, 0))} {flags.techDetails ? 'prompts' : 'requests'} and {count(activity.days.reduce((n, d) => n + d.images, 0))} {flags.techDetails ? 'images' : 'pictures'} in the last year{:else}Activity{/if}
				</h2>
				<div class="cal" bind:this={gridEl} bind:clientWidth={calWidth} style:--cell="{cellSize}px" role="img" aria-label="Activity per day for the last 53 weeks. {days(streaks.days)} with activity.">
					<div class="months num" aria-hidden="true">
						{#each monthMarks as mark, i (i)}<span>{mark}</span>{/each}
					</div>
					<div class="rows">
						<div class="wd num" aria-hidden="true"><span></span><span>Mon</span><span></span><span>Wed</span><span></span><span>Fri</span><span></span></div>
						<!-- svelte-ignore a11y_no_static_element_interactions -->
						<div class="weeks" onmouseleave={() => (tip = null)}>
							{#each weeks as column, w (w)}
								<div class="week">
									{#each column as cell (cell.day)}
										<span class="cell l{cell.level}" class:future={cell.future} class:today={cell.day === today} onmouseenter={(e) => hover(e, cell)}></span>
									{/each}
								</div>
							{/each}
						</div>
					</div>
					{#if tip}
						<div class="tip num" style:left="{tip.x}px" style:top="{tip.y}px">
							<b>{dayName(tip.cell.day)}</b>
							{#if tip.cell.data}
								{count(tip.cell.data.prompts)} {tip.cell.data.prompts === 1 ? (flags.techDetails ? 'prompt' : 'request') : flags.techDetails ? 'prompts' : 'requests'} · {count(tip.cell.data.images)} {tip.cell.data.images === 1 ? 'picture' : 'pictures'}{flags.techDetails ? ` · ${count(tip.cell.data.tokens_out)} tokens out` : ''}
							{:else}
								nothing
							{/if}
						</div>
					{/if}
					<p class="scale num" aria-hidden="true">
						<span>{flags.techDetails ? 'Days are UTC. Shade is prompts plus images.' : 'Each square is a day; darker days had more requests and pictures.'}</span>
						<span class="ends">less <span class="cell l0"></span><span class="cell l1"></span><span class="cell l2"></span><span class="cell l3"></span><span class="cell l4"></span> more</span>
					</p>
				</div>

				<dl class="totals" style:--cols={totals.length}>
					{#each totals as total (total.label)}
						<div title={total.title ?? ''}>
							<dt class="label">{total.label}</dt>
							<dd class="num big selectable">{usage ? total.value(usage.all_time) : activity ? total.value(activity.all_time) : '–'}</dd>
							{#if usage}
								<dd class="num small">today {total.value(usage.today)}</dd>
								<dd class="num small">7 days {total.value(usage.last_7_days)}</dd>
							{/if}
						</div>
					{/each}
				</dl>
			</section>

			<section class="side" aria-label="Streaks, models and sending">
				<h2 class="rule label">Streaks</h2>
				<dl class="pairs">
					<div><dt>Current</dt><dd class="num">{days(streaks.current)}</dd></div>
					<div><dt>Longest</dt><dd class="num">{days(streaks.longest)}</dd></div>
					<div><dt>Days with activity</dt><dd class="num">{streaks.days}</dd></div>
					<div><dt>Busiest day</dt><dd class="num">{streaks.busiest ? `${dayName(streaks.busiest.day).replace(/ \d{4}$/, '')} · ${weight(streaks.busiest)}${flags.techDetails ? '' : ' requests'}` : 'none yet'}</dd></div>
				</dl>

				<h2 class="rule label gap">Most used models</h2>
				{#if used.length > 0}
					<ul class="used">
						{#each used as item (item.id)}
							<li>
								<button class="uname" onclick={() => app.showModel(item.id)} title="Show in Models">{nameOf(item.id)}</button>
								<span class="ubar"><span style:width="{(item.prompts / usedPeak) * 100}%"></span></span>
								<span class="num">{count(item.prompts)}</span>
							</li>
						{/each}
					</ul>
					<p class="foot">{flags.techDetails ? 'Prompts sent to each model in the last year.' : 'Messages and pictures asked of each model in the last year.'}</p>
				{:else}
					<p class="foot">No prompts yet.</p>
				{/if}

				<h2 class="rule label gap">Usage data <Hint topic="data" /></h2>
				{#if usage}
					<dl class="pairs" aria-live="polite">
						<div><dt>Waiting to be sent</dt><dd class="num">{count(usage.sync.pending)} {usage.sync.pending === 1 ? 'event' : 'events'}</dd></div>
						<div><dt>Last sent</dt><dd class="num">{usage.sync.last_sent_ms ? ago(usage.sync.last_sent_ms, now) : 'nothing sent yet'}</dd></div>
						{#if activity && flags.techDetails}<div><dt>Install</dt><dd class="num selectable" title={activity.install_id}>{activity.install_id.slice(0, 8)}</dd></div>{/if}
					</dl>
					{#if (syncProblem || usage.sync.last_error) && usage.sync.pending > 0}
						<p class="foot problem selectable" role="alert">Not sent: {syncProblem || usage.sync.last_error}. Rigbrew keeps trying.</p>
					{/if}
					<p class="acts">
						<button class="btn" onclick={sendNow} disabled={sending || usage.sync.pending === 0}><Icon name="sync" size={14} /> {sending ? 'Sending…' : 'Send now'}</button>
						<button class="btn quiet" onclick={() => app.openPrivacy()}>{flags.techDetails ? 'See the queue' : 'See what is sent'}</button>
					</p>
				{:else}
					<p class="foot">Usage figures are not available yet.</p>
				{/if}
			</section>

			<section class="recent" aria-label="Recent activity">
				<h2 class="rule label">Recent activity</h2>
				{#if recent.length > 0}
					<ol class="log selectable">
						{#each recent as item (item.id)}
							<li>
								<time class="num" datetime={new Date(item.at).toISOString()} title={new Date(item.at).toLocaleString()}>{now - item.at < DAY ? clock(item.at).slice(0, 5) : ago(item.at, now)}</time>
								<span class="what">{item.what}</span>
								<span class="detail" class:num={flags.techDetails || /^[\d.,×\s·sGBM]+$|^in /.test(item.detail)}>{item.detail}</span>
							</li>
						{/each}
					</ol>
					<p class="foot">From this {machine}’s own log.</p>
				{:else if recentDays.length > 0}
					<ol class="log days selectable">
						{#each recentDays as d (d.day)}
							<li>
								<time class="num">{dayName(d.day).replace(/ \d{4}$/, '')}</time>
								<span class="what">{count(d.prompts)} {d.prompts === 1 ? 'prompt' : 'prompts'}, {count(d.images)} {d.images === 1 ? 'image' : 'images'}</span>
								<span class="num detail">{flags.techDetails ? `${count(d.tokens_out)} tokens out` : ''}</span>
							</li>
						{/each}
					</ol>
				{:else}
					<p class="foot">Nothing yet. Send a message and it shows up here.</p>
				{/if}
				{#if usage?.disk}
					<h2 class="rule label gap">On disk</h2>
					<ol class="log disk selectable">
						{#each usage.disk.packages as item (item.id + item.variant)}
							<li>
								<span class="what"><button class="uname" onclick={() => app.showModel(item.id)}>{nameOf(item.id)}</button> <span class="muted" class:num={flags.buildIds}>{diskVersion(item.id, item.variant)}</span></span>
								<span class="num detail">{bytes(item.bytes)}</span>
							</li>
						{/each}
					</ol>
					<p class="foot"><span class="num">{bytes(usage.disk.total_bytes)}</span> of models, <span class="num">{size(usage.disk.free_bytes / 1024 / 1024)}</span> free on this disk.</p>
				{/if}
			</section>
		</div>
	</div>
</section>

<style>
	.body {
		flex: 1;
		min-height: 0;
		padding: 16px var(--gutter) 28px;
	}

	.grid {
		display: grid;
		grid-template-columns: minmax(0, 1fr);
		grid-template-areas: 'id' 'year' 'side' 'recent';
		gap: 14px 36px;
	}

	@media (min-width: 1040px) {
		.grid {
			grid-template-columns: minmax(0, 1fr) minmax(280px, 340px);
			grid-template-areas: 'id id' 'year side' 'recent side';
			grid-template-rows: auto auto 1fr;
		}
	}

	@media (min-width: 1560px) {
		.grid {
			grid-template-columns: minmax(0, 940px) minmax(300px, 420px) minmax(300px, 1fr);
			grid-template-areas: 'id id id' 'year side recent';
			grid-template-rows: auto 1fr;
		}
	}

	/* --- who --- */
	.id {
		grid-area: id;
		display: grid;
		grid-template-columns: auto minmax(0, 1fr) auto;
		grid-template-areas: 'avatar who sign' 'avatar say say';
		align-items: center;
		gap: 2px 14px;
		padding-bottom: 14px;
		border-bottom: 1px solid var(--line);
	}

	.avatar {
		grid-area: avatar;
		display: grid;
		place-items: center;
		width: 52px;
		height: 52px;
		border-radius: 4px;
		box-shadow: inset 0 0 0 1px var(--line);
		background: var(--well);
		color: var(--muted);
		font: 600 22px/1 var(--rb-display);
	}

	.avatar.named {
		box-shadow: none;
		background: var(--ink);
		color: var(--bg);
	}

	.who {
		grid-area: who;
		min-width: 0;
	}

	.who h1 {
		font-size: 19px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}

	.meta {
		font-size: 12px;
		color: var(--muted);
	}

	.uid {
		font: inherit;
		color: var(--ink);
		padding: 0 4px;
		margin-left: -4px;
		border-radius: 3px;
		box-shadow: inset 0 0 0 1px var(--line);
	}

	.uid:hover {
		background: var(--hover);
	}

	.plainmeta {
		font-family: var(--rb-text);
		font-size: 12.5px;
	}

	.sep::before {
		content: '·';
		margin: 0 7px;
		color: var(--faint);
	}

	/* --- the year --- */
	.year {
		grid-area: year;
		min-width: 0;
	}

	.cal {
		position: relative;
		margin-top: 6px;
	}

	.months {
		display: grid;
		grid-template-columns: repeat(53, calc(var(--cell) + 2px));
		margin-left: 30px;
		height: 14px;
		font-size: 10.5px;
		color: var(--muted);
	}

	.months span {
		white-space: nowrap;
		overflow: visible;
	}

	.rows {
		display: flex;
	}

	.wd {
		flex: none;
		width: 30px;
		display: grid;
		grid-template-rows: repeat(7, calc(var(--cell) + 2px));
		font-size: 10px;
		line-height: calc(var(--cell) + 2px);
		color: var(--faint);
	}

	.weeks {
		display: flex;
	}

	.week {
		display: grid;
		grid-template-rows: repeat(7, calc(var(--cell) + 2px));
		width: calc(var(--cell) + 2px);
	}

	.cell {
		display: block;
		width: var(--cell);
		height: var(--cell);
		border-radius: 1.5px;
		background: color-mix(in srgb, var(--line) 55%, var(--bg));
	}

	.cell.l1 {
		background: color-mix(in srgb, var(--accent) 28%, var(--bg));
	}

	.cell.l2 {
		background: color-mix(in srgb, var(--accent) 52%, var(--bg));
	}

	.cell.l3 {
		background: color-mix(in srgb, var(--accent) 76%, var(--bg));
	}

	.cell.l4 {
		background: var(--accent);
	}

	.cell.future {
		background: transparent;
		box-shadow: inset 0 0 0 1px var(--line-soft);
	}

	.cell.today {
		outline: 1px solid var(--ink);
		outline-offset: 1px;
	}

	.weeks .cell:not(.future):hover {
		outline: 1px solid var(--ink);
		outline-offset: 0;
	}

	.tip {
		position: absolute;
		z-index: 5;
		transform: translate(-50%, calc(-100% - 6px));
		padding: 4px 8px;
		border-radius: 3px;
		background: var(--ink);
		color: var(--bg);
		font-size: 11px;
		line-height: 1.5;
		white-space: nowrap;
		pointer-events: none;
	}

	.tip b {
		display: block;
		font-weight: 500;
	}

	.scale {
		display: flex;
		justify-content: space-between;
		gap: 12px;
		margin: 8px 0 0 30px;
		max-width: calc(53 * (var(--cell) + 2px));
		font-size: 10.5px;
		color: var(--faint);
	}

	.ends {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		white-space: nowrap;
	}

	.ends .cell {
		width: 9px;
		height: 9px;
	}

	.totals {
		display: grid;
		grid-template-columns: repeat(var(--cols, 4), minmax(0, 1fr));
		margin-top: 18px;
		border-top: 1px solid var(--line);
		border-bottom: 1px solid var(--line);
	}

	.totals div {
		padding: 8px 12px 9px;
		border-left: 1px solid var(--line-soft);
		min-width: 0;
	}

	.totals div:first-child {
		padding-left: 0;
		border-left: 0;
	}

	.big {
		margin-top: 2px;
		font-size: 21px;
		line-height: 1.2;
		letter-spacing: -0.03em;
		white-space: nowrap;
	}

	.small {
		font-size: 11px;
		color: var(--muted);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	/* --- side --- */
	.side {
		grid-area: side;
		min-width: 0;
	}

	.gap {
		margin-top: 16px;
	}

	.pairs div {
		display: flex;
		justify-content: space-between;
		align-items: center;
		gap: 12px;
		height: 24px;
		border-bottom: 1px solid var(--line-soft);
	}

	.pairs dt {
		color: var(--muted);
		white-space: nowrap;
	}

	.pairs dd {
		font-size: 12px;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.used li {
		display: grid;
		grid-template-columns: minmax(96px, 150px) minmax(0, 1fr) 34px;
		align-items: center;
		gap: 10px;
		height: 24px;
		border-bottom: 1px solid var(--line-soft);
	}

	.uname {
		font: 600 12px/1 var(--rb-display);
		letter-spacing: -0.045em;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.uname:hover {
		color: var(--accent);
	}

	.ubar {
		height: 6px;
		background: var(--line-soft);
	}

	.ubar span {
		display: block;
		height: 100%;
		background: var(--accent);
	}

	.used .num {
		font-size: 12px;
		text-align: right;
	}

	.foot {
		margin-top: 6px;
		font-size: 12px;
		color: var(--muted);
	}

	.foot.problem {
		color: var(--fail);
	}

	.foot .num {
		color: var(--ink);
	}

	.acts {
		display: flex;
		gap: 6px;
		margin-top: 8px;
	}

	/* --- recent --- */
	.recent {
		grid-area: recent;
		min-width: 0;
	}

	.log li {
		display: grid;
		grid-template-columns: 58px minmax(0, auto) minmax(0, 1fr);
		align-items: center;
		gap: 10px;
		height: 24px;
		border-bottom: 1px solid var(--line-soft);
		white-space: nowrap;
	}

	.log.days li {
		grid-template-columns: 84px minmax(0, auto) minmax(0, 1fr);
	}

	.log.disk li {
		grid-template-columns: minmax(0, 1fr) auto;
	}

	.log time {
		font-size: 11.5px;
		color: var(--faint);
	}

	.what {
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.detail {
		font-size: 11.5px;
		color: var(--muted);
		text-align: right;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	@media (max-width: 860px) {
		.id {
			grid-template-columns: auto minmax(0, 1fr);
			grid-template-areas: 'avatar who' 'sign sign' 'say say';
		}

		.totals {
			grid-template-columns: repeat(2, minmax(0, 1fr));
		}

		.totals div:nth-child(3) {
			padding-left: 0;
			border-left: 0;
		}

		.totals div:nth-child(n + 3) {
			border-top: 1px solid var(--line-soft);
		}
	}
</style>
