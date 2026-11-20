<script lang="ts">
	import { onMount } from 'svelte';
	import { app } from '#lib/state.svelte.ts';
	import * as api from '#lib/api.ts';
	import { ago, clock, day } from '#lib/format.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import Hint from '#lib/components/Hint.svelte';
	import type { QueuedEvent } from '#lib/types.ts';
	import Icon from '#lib/components/Icon.svelte';

	let events = $state<QueuedEvent[]>([]);
	let count = $state(0);
	let loaded = $state(false);
	let problem = $state('');
	let confirming = $state(false);
	let withdrawing = $state(false);

	async function refresh() {
		try {
			const pending = await api.pendingEvents();
			count = pending.count;
			events = pending.events;
			problem = '';
		} catch (e) {
			problem = e instanceof Error ? e.message : String(e);
		}
		loaded = true;
	}

	onMount(() => {
		refresh();
		app.refreshUsage();
		const timer = setInterval(() => {
			if (app.link === 'up') refresh();
		}, 2000);
		return () => clearInterval(timer);
	});

	async function withdraw() {
		withdrawing = true;
		try {
			await app.withdraw();
		} catch (e) {
			problem = e instanceof Error ? e.message : String(e);
			withdrawing = false;
			confirming = false;
		}
	}

	interface PromptPayload {
		package?: string;
		variant?: string;
		messages?: Said[];
	}

	interface Said {
		role: string;
		content: unknown;
		tool_calls?: { function?: { name?: string; arguments?: string } }[];
	}

	/** What a logged message said, including a call to another model. */
	function text(message: Said) {
		const calls = (message.tool_calls ?? []).map((c) => `${c.function?.name ?? 'tool'}(${c.function?.arguments ?? ''})`).join(' ');
		const content = typeof message.content === 'string' ? message.content : message.content == null ? '' : JSON.stringify(message.content);
		return [content, calls].filter(Boolean).join(' ');
	}

	/** Whether an event carries what the person wrote. Older services do not say: prompts and answers do. */
	const carries = (e: QueuedEvent) => e.content ?? (e.kind === 'prompt' || e.kind === 'response');
	const withContent = $derived(events.filter(carries).length);

	function fields(payload: unknown): [string, string][] {
		if (!payload || typeof payload !== 'object') return [];
		return Object.entries(payload as Record<string, unknown>)
			.filter(([, v]) => v !== null && v !== undefined)
			.map(([k, v]) => [k.replaceAll('_', ' '), typeof v === 'object' ? JSON.stringify(v) : typeof v === 'number' ? String(+v.toFixed(2)) : String(v)]);
	}
</script>

<section class="layers">
	<div>
		<h2 class="label"><span class="kind content">content</span> What you write, with your consent</h2>
		<p class="muted">The text of your prompts and of the answers, picture descriptions, and the link to your account. Collected only while your consent stands.</p>
	</div>
	<div>
		<h2 class="label"><span class="kind">basic</span> Basic usage data, always</h2>
		<p class="muted">Screens and main buttons, models installed and loaded, counts, speeds, errors, hardware class, app version. Never what you type or what a model answers; a random install number, no account.</p>
	</div>
	<Hint topic="data" />
</section>

<section class="consent">
	<div>
		<h2 class="label">Consent</h2>
		{#if app.consent}
			<p>Accepted on {day(app.consent.accepted_at_ms)}. Text version <span class="num selectable">{app.consent.version}</span>.</p>
		{/if}
		<p class="muted">Withdrawing stops the collection of what you write and removes it from the events below. Basic usage data continues. Rigbrew waits until you accept again.</p>
		<p class="muted docs"><button class="link" onclick={() => guide.openLegal('privacy', 'privacy_view')}>Privacy Policy</button> · <button class="link" onclick={() => guide.openLegal('terms', 'privacy_view')}>Terms of Use</button></p>
	</div>
	{#if confirming}
		<div class="confirm">
			<button class="btn danger" onclick={withdraw} disabled={withdrawing}>Withdraw{withContent > 0 ? ` and remove content from ${withContent} ${withContent === 1 ? 'event' : 'events'}` : ''}</button>
			<button class="btn quiet" onclick={() => (confirming = false)} disabled={withdrawing}>Keep</button>
		</div>
	{:else}
		<button class="btn" onclick={() => (confirming = true)}>Withdraw consent</button>
	{/if}
</section>

<section>
	<header class="queue-head">
		<h2 class="label">Waiting to be sent</h2>
		<span class="num count">{count}</span>
		<span class="live num"><span class="dot"></span>live</span>
	</header>
	<p class="muted lead">
		{#if app.usage?.sync.last_sent_ms}Events were last sent {ago(app.usage.sync.last_sent_ms)}.{:else}Nothing has been sent from this machine yet.{/if}
		This list is exactly what goes next, newest first. Events marked <span class="kind content">content</span> carry what you wrote; the others are basic usage data.
		{#if count > events.length}Showing the newest {events.length} of {count}.{/if}
	</p>

	{#if problem}
		<p class="problem" role="alert">Could not read the queue. {problem}</p>
	{/if}

	{#if loaded && events.length === 0 && !problem}
		<p class="muted">The queue is empty.</p>
	{/if}

	<ol class="events selectable">
		{#each events as event (event.id)}
			{@const prompt = event.kind === 'prompt' && Array.isArray((event.payload as PromptPayload)?.messages) ? (event.payload as PromptPayload) : null}
			<li>
				<time class="num" datetime={new Date(event.ts_ms).toISOString()}>{clock(event.ts_ms)}</time>
				<span class="what">
					<span class="kind" class:content={carries(event)} title={carries(event) ? 'Carries what you wrote: sent only with your consent' : 'Basic usage data: no text you wrote'}>{carries(event) ? 'content' : 'basic'}</span>
					<code>{event.kind}</code>
				</span>
				<div class="payload">
					{#if prompt}
						{#each prompt.messages ?? [] as message, i (i)}
							<p class="said"><span>{message.role}</span>{text(message)}</p>
						{/each}
						<p class="kv"><span>package</span>{prompt.package} <span>variant</span>{prompt.variant}</p>
					{:else if fields(event.payload).length > 0}
						<p class="kv">
							{#each fields(event.payload) as [key, value] (key)}<span>{key}</span>{value} {/each}
						</p>
					{/if}
				</div>
			</li>
		{/each}
	</ol>

	{#if events.length > 0}
		<p class="muted ids selectable">
			Every event carries the installation number <code>{events[0].install_id}</code> and a session number. Only content events are linked to your account{events.some((e) => e.user_id) ? '' : ', once you sign in'}.
		</p>
	{/if}
</section>

<style>
	section {
		padding: 10px 0;
	}

	.layers {
		display: grid;
		grid-template-columns: minmax(0, 1fr) minmax(0, 1fr) auto;
		gap: 16px;
		border-bottom: 1px solid var(--line);
	}

	.layers p {
		margin-top: 2px;
		font-size: 12.5px;
	}

	.layers h2 {
		display: flex;
		align-items: center;
		gap: 6px;
		white-space: normal;
	}

	/* Which layer an event belongs to. */
	.kind {
		display: inline-flex;
		align-items: center;
		height: 16px;
		padding: 0 4px;
		border-radius: var(--r-s);
		box-shadow: inset 0 0 0 1px var(--line);
		font: 400 11px/16px var(--rb-label);
		text-transform: uppercase;
		color: var(--muted);
		white-space: nowrap;
	}

	.kind.content {
		box-shadow: none;
		background: var(--accent-soft);
		color: var(--ink);
	}

	.what {
		display: flex;
		align-items: center;
		gap: 6px;
		min-width: 0;
	}

	.consent {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 16px;
		border-bottom: 1px solid var(--line);
	}

	.consent p {
		margin-top: 2px;
	}

	.confirm {
		flex: none;
		display: flex;
		gap: 6px;
	}

	.num {
		font-size: 12px;
	}

	.queue-head {
		display: flex;
		align-items: center;
		gap: 8px;
	}

	.count {
		color: var(--ink);
	}

	.live {
		margin-left: auto;
		display: inline-flex;
		align-items: center;
		gap: 6px;
		font-size: 11px;
		color: var(--muted);
	}

	.dot {
		width: 6px;
		height: 6px;
		background: var(--fit);
		animation: blink 1.4s steps(2, jump-none) infinite;
	}

	.lead {
		margin: 2px 0 8px;
		font-size: 12.5px;
	}

	.events li {
		display: grid;
		grid-template-columns: 62px 210px minmax(0, 1fr);
		gap: 10px;
		padding: 4px 0;
		border-top: 1px solid var(--line-soft);
		font-size: 12px;
		line-height: 18px;
	}

	.events time {
		color: var(--faint);
	}

	code {
		font: 12px/18px var(--rb-data);
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.payload {
		min-width: 0;
		color: var(--muted);
		overflow-wrap: anywhere;
	}

	.said {
		color: var(--ink);
		display: -webkit-box;
		-webkit-box-orient: vertical;
		-webkit-line-clamp: 3;
		line-clamp: 3;
		overflow: hidden;
	}

	.said span,
	.kv span {
		margin-right: 6px;
		font: 400 11px/18px var(--rb-label);
		text-transform: uppercase;
		color: var(--faint);
	}

	.kv {
		font-family: var(--rb-data);
		font-size: 11.5px;
	}

	.kv span:not(:first-child) {
		margin-left: 8px;
	}

	.ids {
		margin-top: 10px;
		font-size: 12px;
	}

	@media (max-width: 700px) {
		.layers {
			grid-template-columns: minmax(0, 1fr);
		}

		.events li {
			grid-template-columns: 62px minmax(0, 1fr);
		}

		.payload {
			grid-column: 1 / -1;
		}
	}
</style>
