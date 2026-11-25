<script lang="ts">
	import { onMount } from 'svelte';
	import { watchFaults } from '#lib/faults.ts';
	import { isTauri } from '#lib/api.ts';
	import { APP_VERSION, app, screens } from '#lib/state.svelte.ts';
	import { chat } from '#lib/chat.svelte.ts';
	import { ux } from '#lib/experience.svelte.ts';
	import { guide } from '#lib/guide.svelte.ts';
	import Help from '#lib/components/Help.svelte';
	import LegalSheet from '#lib/components/LegalSheet.svelte';
	import Mark from '#lib/components/Mark.svelte';
	import Palette from '#lib/components/Palette.svelte';
	import Sidebar from '#lib/components/Sidebar.svelte';
	import UpdateNotice from '#lib/components/UpdateNotice.svelte';
	import Chat from '#lib/screens/Chat.svelte';
	import Code from '#lib/screens/Code.svelte';
	import Device from '#lib/screens/Device.svelte';
	import Models from '#lib/screens/Models.svelte';
	import Profile from '#lib/screens/Profile.svelte';
	import Settings from '#lib/screens/Settings.svelte';
	import Welcome from '#lib/screens/Welcome.svelte';

	// The shell: a sidebar (new chat, the places, the chats, the account) and the current screen.
	// Settings and Help are sheets over it, the palette a layer over everything.

	onMount(() => {
		const root = document.documentElement;
		if (isTauri) root.dataset.native = '';
		root.dataset.platform = navigator.platform.toLowerCase().includes('mac') ? 'mac' : navigator.platform.toLowerCase().includes('win') ? 'windows' : 'other';
		app.applyTheme();
		ux.applyTextSize();
		app.boot();
		return watchFaults();
	});

	// Keep a valid chat model selected as installs come and go, whatever screen is open.
	$effect(() => {
		const models = app.chatPackages;
		if (models.length > 0 && !models.some((m) => m.id === chat.model)) chat.model = models[0].id;
	});

	// With only an image model installed there is nothing to chat with: stay in Image mode.
	$effect(() => {
		if (app.chatPackages.length === 0 && app.imagePackage && chat.mode !== 'image') chat.mode = 'image';
	});

	function onkeydown(event: KeyboardEvent) {
		if (app.phase !== 'ready' || app.update === 'hard') return;
		if (event.key === 'Escape' && !event.defaultPrevented) {
			if (app.palette) {
				event.preventDefault();
				app.palette = false;
			} else if (guide.helpOpen) {
				event.preventDefault();
				guide.closeHelp();
			} else if (app.sheet) {
				event.preventDefault();
				app.closeSheet();
			}
			return;
		}
		// Help: F1, or Cmd/Ctrl+/ (also with Shift, which is "?" on many layouts).
		if (event.key === 'F1' || ((event.metaKey || event.ctrlKey) && (event.key === '/' || event.key === '?'))) {
			event.preventDefault();
			app.palette = false;
			if (guide.helpOpen) guide.closeHelp();
			else guide.openHelp('', 'shortcut');
			return;
		}
		if (!(event.metaKey || event.ctrlKey) || event.altKey) return;
		const key = event.key.toLowerCase();
		if (event.shiftKey) {
			if (key === 'i' && app.packages.some((p) => p.modality === 'image')) {
				// Switch the composer between Chat and Image.
				event.preventDefault();
				app.go('chat');
				chat.setMode(chat.mode === 'image' ? 'chat' : 'image', 'shortcut');
			} else if (key === 'u' && chat.engine && !chat.busy) {
				event.preventDefault();
				app.track('unload_clicked', { from: 'shortcut' });
				const name = app.packages.find((p) => p.id === chat.engine?.package)?.name ?? 'the model';
				chat
					.unload()
					.then(() => (app.say(`Unloaded ${name}.`), app.refreshSystem()))
					.catch(() => chat.syncEngine());
			}
			return;
		}
		if (key === 'k') {
			event.preventDefault();
			app.palette = !app.palette;
		} else if (key === ',') {
			// Settings, as everywhere on the Mac.
			event.preventDefault();
			if (app.sheet) app.closeSheet();
			else app.openSettings();
		} else if (key === 'n') {
			// New chat, from anywhere.
			event.preventDefault();
			chat.newChat();
			app.go('chat');
		} else if (key === '\\') {
			event.preventDefault();
			app.togglePane();
		} else if (key >= '1' && key <= String(screens.length)) {
			event.preventDefault();
			app.go(screens[Number(key) - 1]);
			if (key === '1') chat.focusRequest++;
		}
	}
</script>

<svelte:window {onkeydown} />

{#if app.phase === 'connecting'}
	<div class="halt" data-tauri-drag-region>
		<Mark size={22} />
		<p class="num wait"><span class="dot"></span>Connecting to the local service</p>
	</div>
{:else if app.phase === 'offline'}
	<div class="halt" data-tauri-drag-region>
		<Mark size={22} />
		<h1>The local service is not running</h1>
		<p class="selectable">Rigbrew needs its background service, rigbrewd, to read the hardware and run models.</p>
		{#if app.problem}<p class="num said selectable">{app.problem}</p>{/if}
		<p><button class="btn primary" onclick={() => app.boot()}>Try again</button></p>
	</div>
{:else if app.update === 'hard'}
	<!-- This version may no longer be used: nothing else is reachable until it is updated. -->
	<div class="halt" data-tauri-drag-region role="alertdialog" aria-labelledby="hard-title" aria-describedby="hard-text">
		<Mark size={22} />
		<h1 id="hard-title">Rigbrew needs an update</h1>
		<p id="hard-text" class="selectable">
			Version <span class="num">{APP_VERSION}</span> can no longer be used.
			{#if app.config?.latest_version}Version <span class="num">{app.config.latest_version}</span> replaces it.{/if}
			Your models and settings stay where they are.
		</p>
		{#if app.config?.notes}<p class="num said selectable">{app.config.notes}</p>{/if}
		<p class="acts">
			{#if app.config?.download_url}
				<button class="btn primary large" onclick={() => app.acceptUpdate()}>Update Rigbrew</button>
			{:else}
				<span>Get the new version from rigbrew.dev.</span>
			{/if}
			<button class="btn quiet" onclick={() => app.loadConfig()} disabled={app.checkingConfig}>{app.checkingConfig ? 'Checking…' : 'Check again'}</button>
		</p>
	</div>
{:else if app.phase === 'welcome'}
	<Welcome />
{:else}
	<div class="shell">
		<Sidebar />
		<div class="stage">
		<div class="grip" data-tauri-drag-region></div>
		<main>
			{#if app.screen === 'chat'}
				<Chat />
			{:else if app.screen === 'code'}
				<Code />
			{:else if app.screen === 'models'}
				<Models />
			{:else if app.screen === 'device'}
				<Device />
			{:else}
				<Profile />
			{/if}
			{#if app.updateNotice}<UpdateNotice />{/if}
			{#if app.sheet}<Settings />{/if}
			{#if guide.helpOpen}<Help />{/if}
		</main>
		{#if app.notice}
			<div class="notice" role="status">
				<p>{app.notice}</p>
				{#if app.noticeAction}
					{@const action = app.noticeAction}
					<button class="act" onclick={() => (app.clearNotice(), action.run())}>{action.label}</button>
				{/if}
			</div>
		{/if}
		</div>
	</div>
	{#if app.palette}<Palette />{/if}
	<LegalSheet />
{/if}

<style>
	.shell {
		height: 100%;
		display: flex;
	}

	.stage {
		position: relative;
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
	}

	/* The top of the content is where the window is dragged from; nothing else lives there. */
	.grip {
		flex: none;
		height: 28px;
	}

	main {
		position: relative;
		flex: 1;
		min-width: 0;
		min-height: 0;
		background: var(--bg);
	}

	/* One line about something that just happened, seated on the bay. */
	.notice {
		flex: none;
		display: flex;
		align-items: center;
		gap: 12px;
		min-height: 26px;
		padding: 3px 6px 3px 12px;
		border-top: 1px solid var(--line);
		background: var(--ink);
		color: var(--bg);
		font-size: 12.5px;
		animation: appear 0.15s var(--ease) both;
	}

	.notice p {
		flex: 1;
		min-width: 0;
	}

	.act {
		flex: none;
		height: 20px;
		padding: 0 8px;
		border-radius: 3px;
		box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--bg) 45%, transparent);
		font-weight: 600;
		color: var(--bg);
	}

	.act:hover {
		background: color-mix(in srgb, var(--bg) 16%, transparent);
	}

	.act:focus-visible {
		outline-color: var(--bg);
	}

	/* Nothing else can be shown: a short, left-aligned statement. */
	.halt {
		height: 100%;
		padding: 76px clamp(24px, 8vw, 96px);
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 8px;
		background: var(--bg);
	}

	.halt h1 {
		margin-top: 14px;
	}

	.halt p {
		max-width: 56ch;
		color: var(--muted);
	}

	.halt .said {
		padding: 6px 10px;
		border-left: 2px solid var(--line);
		font-size: 12px;
	}

	.halt .acts {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 8px;
	}

	.wait {
		display: flex;
		align-items: center;
		gap: 8px;
		margin-top: 14px;
		font-size: 12px;
	}

	.dot {
		width: 6px;
		height: 6px;
		background: var(--accent);
		animation: blink 0.9s steps(2, jump-none) infinite;
	}
</style>
