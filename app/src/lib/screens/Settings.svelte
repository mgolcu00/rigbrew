<script lang="ts">
	import { onMount } from 'svelte';
	import { ACCOUNTS, AccountDataLine, getAccount } from '#account';
	import { APP_VERSION, app, type Theme } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { LEVELS, LEVEL_ORDER, TEXT_SIZES, ux } from '#lib/experience.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import { ago, bytes, variantName } from '#lib/format.ts';
	import { machine, nameById, versionLabel } from '#lib/words.ts';
	import Hint from '#lib/components/Hint.svelte';
	import Icon from '#lib/components/Icon.svelte';
	import Privacy from './Privacy.svelte';

	// Settings: a short list on a sheet over the app. One line per thing. Privacy is its
	// second face, behind a quiet line at the bottom; export and delete share one line there.

	let now = $state(Date.now());
	onMount(() => {
		app.refreshUsage();
		const timer = setInterval(() => (now = Date.now()), 5000);
		return () => clearInterval(timer);
	});

	const themes: { id: Theme; name: string }[] = [
		{ id: 'system', name: 'System' },
		{ id: 'light', name: 'Light' },
		{ id: 'dark', name: 'Dark' }
	];

	const disk = $derived(app.usage?.disk ?? null);
	const flags = $derived(ux.flags);
	const nameOf = (id: string) => nameById(app.packages, id, flags);
	const versionOf = (id: string, variant: string) => {
		const pkg = app.packages.find((p) => p.id === id);
		return pkg ? versionLabel(pkg, variant, flags) : variantName(variant);
	};
	const account = $derived(getAccount());

	let clearing = $state<'idle' | 'asking' | 'busy'>('idle');
	async function deleteAllChats() {
		clearing = 'busy';
		try {
			const n = await chat.removeAll();
			app.say(`Deleted ${n} ${n === 1 ? 'chat' : 'chats'} and their pictures.`);
		} catch (e) {
			app.say(`Could not delete every chat. ${e instanceof Error ? e.message : String(e)}`);
		} finally {
			clearing = 'idle';
		}
	}
</script>

<!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
<div class="veil" onclick={() => app.closeSheet()}>
	<div class="sheet" class:wide={app.sheet === 'privacy'} role="dialog" tabindex="-1" aria-modal="true" aria-label={app.sheet === 'privacy' ? 'Privacy and data' : 'Settings'} onclick={(e) => e.stopPropagation()}>
		<header>
			{#if app.sheet === 'privacy'}
				<button class="btn quiet back" onclick={() => (app.sheet = 'settings')}><Icon name="back" size={14} /> Settings</button>
				<h1>Privacy and data</h1>
			{:else}
				<h1>Settings</h1>
			{/if}
			<span class="grow"></span>
			<button class="btn quiet" onclick={() => app.closeSheet()} aria-label="Close">Close{#if flags.keyHints}&nbsp;<kbd>esc</kbd>{/if}</button>
		</header>

		{#if app.sheet === 'privacy'}
			<div class="content scroll"><Privacy /></div>
		{:else}
			<div class="content scroll">
				<dl class="list">
					<div>
						<dt>How much to show <Hint topic="levels" /></dt>
						<dd>
							<div class="levels" role="radiogroup" aria-label="How much to show">
								{#each LEVEL_ORDER as level (level)}
									<button role="radio" aria-checked={ux.level === level} onclick={() => app.setLevel(level, 'settings')}>
										<span class="lname">{LEVELS[level].name}</span>
										<span class="lline">{LEVELS[level].line}</span>
									</button>
								{/each}
							</div>
						</dd>
					</div>

					<div>
						<dt>Text size</dt>
						<dd>
							<div class="seg" role="radiogroup" aria-label="Text size">
								{#each TEXT_SIZES as t (t.id)}
									<button role="radio" aria-checked={ux.textSize === t.id} onclick={() => app.setTextSize(t.id, 'settings')} style:font-size="{12.5 * t.scale}px">{t.name}</button>
								{/each}
							</div>
						</dd>
					</div>

					<div>
						<dt>Appearance</dt>
						<dd>
							<div class="seg" role="radiogroup" aria-label="Appearance">
								{#each themes as theme (theme.id)}
									<button role="radio" aria-checked={app.theme === theme.id} onclick={() => app.setTheme(theme.id)}>{theme.name}</button>
								{/each}
							</div>
						</dd>
					</div>

					<div>
						<dt>Updates</dt>
						<dd>
							<span class="line">
								<span>
									Rigbrew <span class="num">{APP_VERSION}</span>
									{#if app.update !== 'none'}
										<span class="sep"><b>{app.config?.latest_version ?? 'a newer version'} is available</b></span>
									{:else if app.config === null}
										<span class="sep muted">update checks are not available yet</span>
									{:else if app.config.stale}
										<span class="sep muted">the update server could not be reached</span>
									{:else}
										<span class="sep muted">up to date{app.configCheckedAt ? `, checked ${ago(app.configCheckedAt, now)}` : ''}</span>
									{/if}
								</span>
								<span class="ctl">
									{#if app.update !== 'none' && app.config?.download_url}<button class="btn primary" onclick={() => app.acceptUpdate()}>Update</button>{/if}
									<button class="btn" onclick={() => app.loadConfig()} disabled={app.checkingConfig}>{app.checkingConfig ? 'Checking…' : 'Check now'}</button>
								</span>
							</span>
							{#if app.update !== 'none' && app.config?.notes}<p class="more selectable">{app.config.notes}</p>{/if}
						</dd>
					</div>

					<div>
						<dt>Chats</dt>
						<dd>
							<span class="line">
								<span>
									{#if chat.persist === 'no'}
										<span class="muted">Kept until Rigbrew quits: this background service cannot save them yet.</span>
									{:else}
										{chat.conversations.length} saved on this {machine}
									{/if}
								</span>
								<span class="ctl">
									{#if clearing === 'asking'}
										<button class="btn danger" onclick={deleteAllChats}>Delete {chat.conversations.length} {chat.conversations.length === 1 ? 'chat' : 'chats'} and their pictures</button>
										<button class="btn quiet" onclick={() => (clearing = 'idle')}>Keep</button>
									{:else}
										<button class="btn" onclick={() => (clearing = 'asking')} disabled={chat.conversations.length === 0 || clearing === 'busy' || chat.busy}>{clearing === 'busy' ? 'Deleting…' : 'Delete all'}</button>
									{/if}
								</span>
							</span>
							{#if clearing === 'asking'}<p class="more">This cannot be undone.</p>{/if}
						</dd>
					</div>

					<div>
						<dt>Help</dt>
						<dd>
							<span class="line">
								<span class="muted">Short answers to common questions, and the four-step tour.</span>
								<span class="ctl">
									<button class="btn" onclick={() => (app.closeSheet(), guide.openHelp('', 'settings'))}>Open help</button>
									<button class="btn quiet" onclick={() => (app.closeSheet(), app.go('chat'), guide.startTour('settings'))}>Take the tour</button>
								</span>
							</span>
						</dd>
					</div>

					<div>
						<dt>Storage</dt>
						<dd>
							{#if disk}
								<span class="line">
									<span><span class="num">{bytes(disk.total_bytes)}</span> in {disk.packages.length} {flags.buildIds ? (disk.packages.length === 1 ? 'build' : 'builds') : disk.packages.length === 1 ? 'model' : 'models'}<span class="sep muted"><span class="num">{bytes(disk.free_bytes)}</span> free</span></span>
									<span class="ctl"><button class="btn" onclick={() => app.go('models')}>Manage in Models</button></span>
								</span>
								{#if disk.packages.length > 0}
									<ul class="builds selectable">
										{#each disk.packages as item (item.id + item.variant)}
											<li><span>{nameOf(item.id)} <span class="muted" class:num={flags.buildIds}>{versionOf(item.id, item.variant)}</span></span><span class="num">{bytes(item.bytes)}</span></li>
										{/each}
									</ul>
								{/if}
								{#if app.system}<p class="more num selectable">{app.system.storage.data_dir}</p>{/if}
							{:else}
								<span class="muted">Disk figures are not available yet.</span>
							{/if}
						</dd>
					</div>

					{#if app.sponsors}
						<div>
							<dt>Support Rigbrew</dt>
							<dd>
								<span class="line">
									<span class="muted">{app.sponsors.headline || 'Rigbrew is free and open source. Sponsors keep it that way.'}</span>
									<span class="ctl"><button class="btn" onclick={() => app.openSponsor('settings')}>Become a sponsor <Icon name="external" size={13} /></button></span>
								</span>
								{#if app.sponsors.tiers.length > 0}
									<p class="more tiers">
										{#each app.sponsors.tiers as tier (tier.name)}
											<button class="link" onclick={() => app.openSponsor('settings', tier.url, tier.name)}>{tier.name} <span class="num">{typeof tier.amount === 'number' ? `$${tier.amount}` : tier.amount}</span></button>
										{/each}
									</p>
								{/if}
							</dd>
						</div>
					{/if}

					{#if ACCOUNTS}
					<div>
						<dt>Account</dt>
						<dd>
							<span class="line">
								<span>{account.label}<span class="sep muted">{account.detail}</span></span>
								<span class="ctl"><button class="btn" onclick={() => app.go('profile')}>Open the profile</button></span>
							</span>
						</dd>
					</div>
					{/if}
				</dl>

				<footer>
					<p><button class="quiet" onclick={() => app.openPrivacy()}>Privacy and data</button></p>
					<AccountDataLine />
				</footer>
			</div>
		{/if}
	</div>
</div>

<style>
	.veil {
		position: absolute;
		inset: 0;
		z-index: 30;
		display: flex;
		justify-content: center;
		align-items: flex-start;
		padding: 28px 16px 16px;
		background: color-mix(in srgb, var(--rb-board) 62%, transparent);
	}

	.sheet {
		width: min(640px, 100%);
		max-height: 100%;
		display: flex;
		flex-direction: column;
		border: 1px solid var(--line);
		border-radius: 6px;
		background: var(--bg);
		box-shadow: 0 18px 50px -14px var(--shade);
		animation: appear 0.14s var(--ease) both;
	}

	.sheet.wide {
		width: min(820px, 100%);
		height: 100%;
	}

	header {
		flex: none;
		display: flex;
		align-items: center;
		gap: 8px;
		height: 38px;
		padding: 0 8px 0 14px;
		border-bottom: 1px solid var(--line);
	}

	header h1 {
		font-size: 13px;
	}

	.back {
		margin-left: -8px;
	}

	.grow {
		flex: 1;
	}

	.content {
		min-height: 0;
		padding: 2px 14px 12px;
	}

	.list > div {
		display: grid;
		grid-template-columns: 132px minmax(0, 1fr);
		gap: 12px;
		padding: 9px 0;
		border-bottom: 1px solid var(--line-soft);
	}

	.list dt {
		padding-top: 3px;
		font-weight: 500;
	}

	.line {
		display: flex;
		align-items: center;
		justify-content: space-between;
		gap: 12px;
		min-height: 24px;
	}

	.line b {
		font-weight: 600;
	}

	.ctl {
		flex: none;
		display: inline-flex;
		gap: 6px;
	}

	.num {
		font-size: 12px;
	}

	.sep::before {
		content: '·';
		margin: 0 7px;
		color: var(--faint);
	}

	.more {
		margin-top: 4px;
		font-size: 12px;
		color: var(--muted);
		overflow-wrap: anywhere;
	}

	.tiers {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 16px;
	}

	.tiers .num {
		color: var(--muted);
	}

	.seg {
		display: inline-flex;
		gap: 1px;
		padding: 1px;
		border-radius: 4px;
		box-shadow: inset 0 0 0 1px var(--line);
	}

	.seg button {
		height: 22px;
		padding: 0 10px;
		border-radius: 3px;
		font-size: 12.5px;
		font-weight: 500;
		color: var(--muted);
	}

	.seg button:hover {
		color: var(--ink);
	}

	.seg button[aria-checked='true'] {
		background: var(--ink);
		color: var(--bg);
	}

	.builds {
		margin-top: 4px;
	}

	/* How much to show: three choices, each with its one line. */
	.levels {
		display: flex;
		flex-direction: column;
		gap: 2px;
	}

	.levels button {
		display: grid;
		grid-template-columns: 76px minmax(0, 1fr);
		align-items: baseline;
		gap: 10px;
		padding: 5px 8px;
		border-radius: var(--r);
		box-shadow: inset 0 0 0 1px transparent;
		text-align: left;
	}

	.levels button:hover {
		background: var(--hover);
	}

	.levels button[aria-checked='true'] {
		box-shadow: inset 0 0 0 1px var(--accent);
		background: var(--accent-soft);
	}

	.lname {
		font-weight: 600;
	}

	.levels button[aria-checked='true'] .lname::before {
		content: '● ';
		color: var(--accent);
	}

	.lline {
		font-size: 12.5px;
		color: var(--muted);
	}

	.levels button[aria-checked='true'] .lline {
		color: var(--ink);
	}

	.builds li {
		display: flex;
		justify-content: space-between;
		gap: 12px;
		height: 22px;
		align-items: center;
		border-top: 1px solid var(--line-soft);
		font-size: 12.5px;
	}

	footer {
		padding-top: 10px;
		display: flex;
		flex-direction: column;
		gap: 2px;
		font-size: 12px;
		color: var(--muted);
	}

	.quiet {
		color: var(--muted);
		border-radius: 2px;
	}

	.quiet:hover {
		color: var(--ink);
		text-decoration: underline;
	}

	@media (max-width: 620px) {
		.list > div {
			grid-template-columns: 1fr;
			gap: 2px;
		}
	}
</style>
