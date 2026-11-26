import { pauseAccount, pushSession, startAccount } from '#account';
import * as api from './api';
import { ux, type Level, type TextSize } from './experience.svelte';
import { size } from './format';
import { guide, type TipId } from './guide.svelte';
import { remembered } from './remember';
import { track as send, type Payload } from './track';
import type { Activity, AppConfig, Consent, Package, System, Usage } from './types';
import { machine, nameOf } from './words';

/** This build's version (package.json). */
export const APP_VERSION: string = __APP_VERSION__;

/** The consent text version, used only when neither the service nor the server names one. */
const CONSENT_FALLBACK = 'draft-2026-11-20.2';

/** The places, plus the profile behind the chip at the bottom left. */
export type Screen = 'chat' | 'code' | 'models' | 'device' | 'profile' | 'studio';
/** In the order of Cmd/Ctrl+1 to 3. */
export const screens: Screen[] = ['chat', 'models', 'device'];

export type Theme = 'system' | 'light' | 'dark';

export interface Install {
	state: 'running' | 'failed';
	variant: string;
	done: number;
	total: number;
	error: string;
	/** The service's job id; empty until the service has answered the pull request. */
	job: string;
}

const pause = (ms: number) => new Promise((r) => setTimeout(r, ms));
const say = (e: unknown) => (e instanceof Error ? e.message : String(e));

/** A button next to the notice line ("Undo"). */
export interface NoticeAction {
	label: string;
	run: () => void;
}

/**
 * Development only: `?config=soft`, `?config=hard` or `?config=none` in the address replaces
 * the server's app config with a made-up one, to see the update notice, the blocking screen
 * and the sponsor section before the server exists. Adding `&consent=<version>` makes that
 * config name another consent text version, to see the app ask for consent again.
 */
function pretendConfig(): AppConfig | null {
	if (!import.meta.env.DEV) return null;
	const params = new URLSearchParams(location.search);
	const kind = params.get('config');
	if (kind !== 'soft' && kind !== 'hard' && kind !== 'none') return null;
	return {
		consent_version: params.get('consent') ?? undefined,
		latest_version: '0.2.0',
		min_version: kind === 'hard' ? '0.2.0' : '0.1.0',
		update: kind,
		download_url: 'https://rigbrew.dev/download',
		notes: 'Image builds start 30% faster. Downloads resume after a restart. Fixes a crash when the disk is full.',
		sponsors: {
			url: 'https://github.com/sponsors/rigbrew',
			headline: 'Rigbrew is free. Sponsors pay for the build machines and the measurements.',
			tiers: [
				{ name: 'Supporter', amount: '$5 a month', url: 'https://github.com/sponsors/rigbrew?tier=supporter' },
				{ name: 'Backer', amount: '$25 a month', url: 'https://github.com/sponsors/rigbrew?tier=backer' },
				{ name: 'Bench sponsor', amount: '$250 a month', url: 'https://github.com/sponsors/rigbrew?tier=bench' }
			]
		},
		stale: false
	};
}

/** Development only: whether this window was put in look mode before a reload. */
function lookedBefore() {
	try {
		return sessionStorage.getItem('rigbrew.look') === '1';
	} catch {
		return false;
	}
}

class AppState {
	/** connecting -> offline | welcome (consent and first scan) | ready */
	phase = $state<'connecting' | 'offline' | 'welcome' | 'ready'>('connecting');
	problem = $state('');
	/** Whether the local service answers. `down` while it is being restarted. */
	link = $state<'up' | 'down'>('up');
	consent = $state<Consent | null>(null);
	/** The consent text version the service expects. */
	consentCurrent = $state('');
	system = $state<System | null>(null);
	packages = $state<Package[]>([]);
	/** Downloads in flight or failed, by package id. */
	installs = $state<Record<string, Install>>({});
	screen = $state<Screen>('chat');
	/** Settings is a sheet over the app; the privacy view is its second face. */
	sheet = $state<null | 'settings' | 'privacy'>(null);
	/** The command palette (Cmd/Ctrl+K). */
	palette = $state(false);
	/** The secondary pane next to the chat: this chat, recent chats, today. */
	pane = $state(remembered.get('pane') !== 'closed');
	/** Appearance: follow the system, or one of the two. */
	theme = $state<Theme>((remembered.get('theme') as Theme | null) ?? 'system');
	/** A build the pointer is on in Models: the bay draws where it would sit. */
	preview = $state<{ package: string; variant: string } | null>(null);
	/** A package Models should scroll to and open (set by the palette). */
	spotlight = $state('');
	/** Per-day activity for the profile; null until asked, or with a service that has none. */
	activity = $state<Activity | null>(null);

	/** The server's app config; null when the service has none to give. */
	config = $state<AppConfig | null>(null);
	configCheckedAt = $state(0);
	checkingConfig = $state(false);
	/** The version whose soft update notice was closed. */
	dismissedUpdate = $state(remembered.get('update.dismissed') ?? '');

	/** Totals, disk and sync status; null when the service cannot give them (yet). */
	usage = $state<Usage | null>(null);
	/** One short line about something that just happened ("Freed 384 MB."); clears itself. */
	notice = $state('');
	noticeAction = $state<NoticeAction | null>(null);
	/** Whether the internet can be reached, as the system reports it. */
	online = $state(typeof navigator === 'undefined' ? true : navigator.onLine);

	chatPackages = $derived(this.packages.filter((p) => p.modality === 'chat' && p.installed.length > 0));
	/** Rigbrew's own tested models: what the Models lists show. */
	catalogue = $derived(this.packages.filter((p) => !p.community));
	/** Models the person added by link. Listed apart, under Community. */
	communityPackages = $derived(this.packages.filter((p) => p.community));

	/** The installed image package, if any: what Image mode and the chat's image tool use. */
	imagePackage = $derived(this.packages.find((p) => p.modality === 'image' && p.installed.length > 0) ?? null);
	/** The installed listening package (dictation), if any. */
	listenPackage = $derived(this.packages.find((p) => p.modality === 'listen' && p.installed.length > 0) ?? null);
	/** The installed video package, if any. */
	videoPackage = $derived(this.packages.find((p) => p.modality === 'video' && p.installed.length > 0) ?? null);
	/** The installed voice package that reads answers aloud, if any. */
	voicePackage = $derived(this.packages.find((p) => p.modality === 'voice' && p.installed.length > 0) ?? null);
	/** An image package this machine can run but has not installed: what the chat offers. */
	imageOffer = $derived(this.packages.find((p) => p.modality === 'image' && p.recommendation.verdict === 'run') ?? null);

	/** The consent text version that counts: the server's when known, else the service's. */
	requiredConsent = $derived(this.config?.consent_version || this.consentCurrent || CONSENT_FALLBACK);

	/**
	 * none | soft (a notice that can be closed) | hard (the app is blocked until updated).
	 * A hard verdict the server could not confirm just now (a stale copy from disk) never
	 * blocks: someone offline keeps a working app and sees the notice.
	 */
	update = $derived.by((): 'none' | 'soft' | 'hard' => {
		const verdict = this.config?.update;
		if (verdict === 'hard') return this.config?.stale ? 'soft' : 'hard';
		return verdict === 'soft' ? 'soft' : 'none';
	});

	updateNotice = $derived(this.update === 'soft' && this.dismissedUpdate !== (this.config?.latest_version ?? 'next'));

	/** What the server says about sponsoring, when it says anything usable. */
	sponsors = $derived.by(() => {
		const s = this.config?.sponsors;
		const tiers = (s?.tiers ?? []).filter((t) => t?.name && /^https?:\/\//.test(t.url ?? ''));
		const url = /^https?:\/\//.test(s?.url ?? '') ? s!.url! : (tiers[0]?.url ?? '');
		return url ? { url, headline: s?.headline ?? '', tiers } : null;
	});

	#started = false;
	#shownUpdate = '';
	#noticeTimer: ReturnType<typeof setTimeout> | undefined;

	/** Shows one short line at the bottom of the window for a few seconds, with an optional action. */
	say(text: string, action: NoticeAction | null = null, ms = 6000) {
		clearTimeout(this.#noticeTimer);
		this.notice = text;
		this.noticeAction = action;
		this.#noticeTimer = setTimeout(() => {
			this.notice = '';
			this.noticeAction = null;
		}, ms);
	}

	clearNotice() {
		clearTimeout(this.#noticeTimer);
		this.notice = '';
		this.noticeAction = null;
	}

	/** Switches the experience level and says once what that changed. */
	setLevel(level: Level, source: 'settings' | 'palette' | 'suggestion') {
		if (level === ux.level) return;
		const from = ux.level;
		ux.setLevel(level);
		guide.forced = null;
		this.say(`${ux.info.change} You can switch back in Settings.`, null, 8000);
		this.track('level_changed', { from, to: level, source });
	}

	setTextSize(size: TextSize, source: 'settings' | 'palette') {
		if (size === ux.textSize) return;
		ux.setTextSize(size);
		this.track('text_size_changed', { size, source });
	}

	/** Bytes an installed build takes on disk: measured by the service when it says, else the catalogue's figure. */
	diskOf(pkg: Package, variant: string): number {
		const measured = this.usage?.disk.packages.find((d) => d.id === pkg.id && d.variant === variant)?.bytes;
		return measured ?? (pkg.variants.find((v) => v.id === variant)?.disk_mb ?? 0) * 1024 * 1024;
	}

	/**
	 * Development only: look at the app without accepting the consent text, for screenshots
	 * and reviews. Nothing is accepted on anyone's behalf; the service keeps only basic usage
	 * data. `?look` in the address, `__view?look=1`, or VITE_RIGBREW_LOOK=1 for the native window.
	 */
	#looking = (import.meta.env.DEV || Boolean(import.meta.env.VITE_RIGBREW_PROBE)) && (import.meta.env.VITE_RIGBREW_LOOK === '1' || new URLSearchParams(location.search).has('look') || lookedBefore());

	get consented() {
		return this.consent !== null && this.consent.version === this.requiredConsent;
	}

	async boot() {
		this.#start();
		this.phase = 'connecting';
		this.problem = '';
		try {
			await this.#read();
		} catch (e) {
			this.problem = say(e);
			this.phase = 'offline';
			return;
		}
		this.link = 'up';
		this.loadConfig();
		this.attachJobs();
		// In the dev preview, http://localhost:1420/?welcome shows the first-launch flow again
		// without withdrawing consent (which would delete the queued events).
		const replay = import.meta.env.DEV && new URLSearchParams(location.search).has('welcome');
		if ((this.consented || this.#looking) && !replay) {
			this.enter();
		} else {
			this.phase = 'welcome';
		}
	}

	async #read() {
		const [consent, system, packages] = await Promise.all([api.consent(), api.system(), api.packages()]);
		this.consent = consent.consent;
		this.consentCurrent = consent.current_version ?? '';
		this.system = system;
		this.packages = packages;
	}

	/** Once: follow the link to the service, keep an eye on downloads, ask for the config now and then. */
	#start() {
		if (this.#started) return;
		this.#started = true;
		if (import.meta.env.DEV) import.meta.hot?.on('rigbrew:view', (view: Record<string, string>) => this.#early(view));
		api.onLink((up) => {
			this.link = up ? 'up' : 'down';
			if (!up) {
				if (this.phase === 'ready') this.tip('service');
				return;
			}
			if (this.phase === 'offline' || this.phase === 'connecting') this.boot();
			else this.#resync();
		});
		// The internet, not the local service: everything still runs, only sending and update checks wait.
		window.addEventListener('offline', () => {
			this.online = false;
			if (this.phase === 'ready') this.tip('offline');
		});
		window.addEventListener('online', () => (this.online = true));
		// Doubles as the heartbeat: a service that is gone is noticed within seconds.
		setInterval(() => {
			if (this.link === 'up' && (this.phase === 'ready' || this.phase === 'welcome')) this.attachJobs();
		}, 5000);
		setInterval(() => this.loadConfig(), 4 * 60 * 60 * 1000);
	}

	/** The service was restarted: it has a new port and token and has forgotten the session. */
	async #resync() {
		try {
			await this.#read();
		} catch {
			return; // gone again; the link listener comes back here
		}
		this.attachJobs();
		this.refreshUsage();
		this.loadConfig();
		if (this.phase === 'ready') {
			pushSession();
			import('./chat.svelte').then(({ chat }) => (chat.syncEngine(), chat.loadList()));
			if (!this.consented && !this.#looking) this.phase = 'welcome';
		}
	}

	/**
	 * Leaves the first-launch flow (or skips it) and opens the app proper. `fresh` is the end of
	 * a real first launch: the short tour follows, once.
	 */
	enter(screen?: Screen, fresh = false) {
		this.phase = 'ready';
		this.track('app_opened', { version: APP_VERSION });
		startAccount();
		this.refreshUsage();
		// Imported lazily: the chat store imports this one.
		import('./chat.svelte').then(({ chat }) => (chat.syncEngine(), chat.loadList()));
		// Chat is what the app is; with nothing installed it says what can be installed.
		this.go(screen ?? 'chat', true);
		if (fresh) guide.startTourOnce();
		// Development only: `?screen=models`, `?sheet=settings`, `?theme=dark`, `?palette` open
		// the app in that state, to look at a screen without clicking through to it. The dev
		// server's `/__view` sends the same to a window that is already open.
		if (import.meta.env.DEV) {
			this.#view(Object.fromEntries(new URLSearchParams(location.search)));
			import.meta.hot?.on('rigbrew:view', (view: Record<string, string>) => this.#view(view));
		}
		// Development only: a build made with VITE_RIGBREW_PROBE takes dev views from, and reports
		// playback measurements to, a small server on this computer (see dev-probe.ts).
		const probe: string | undefined = import.meta.env.VITE_RIGBREW_PROBE;
		if (probe) import('./dev-probe.ts').then(({ startProbe }) => startProbe(probe, (view) => this.#view(view)));
	}

	/**
	 * Development only. Besides screen, sheet, theme and palette: `level=guided|standard|pro`,
	 * `text=small|default|large|larger`, `tour=0..3` (`tour=off`), `help=<article>` (`help=off`),
	 * `tip=<id>`, `suggest=standard|pro` (`suggest=off`), `fail=<package>` (a failed download),
	 * `pane=open|closed`, `chat=new|<id>`, `checklist=reset`. Studio: `film=<id>` opens a film, then
	 * `play` presses Play film or `export` exports it.
	 */
	#view(view: Record<string, string>) {
		const wanted = view.screen as Screen | undefined;
		if (wanted && ['chat', 'code', 'models', 'device', 'profile', 'studio'].includes(wanted)) this.screen = wanted;
		if (view.film) {
			import('./studio.svelte').then(async ({ studio }) => {
				await studio.open(view.film);
				if ('export' in view) studio.exportFilm();
				else if ('play' in view) studio.playFilm();
			});
		}
		if (view.sheet === 'settings' || view.sheet === 'privacy') this.sheet = view.sheet;
		else if ('sheet' in view) this.sheet = null;
		if (view.theme === 'light' || view.theme === 'dark' || view.theme === 'system') {
			this.theme = view.theme;
			this.applyTheme();
		}
		if ('palette' in view) this.palette = view.palette !== 'off';
		if (view.level === 'guided' || view.level === 'standard' || view.level === 'pro') ux.setLevel(view.level);
		if (['small', 'default', 'large', 'larger'].includes(view.text)) ux.setTextSize(view.text as TextSize);
		if ('tour' in view) {
			if (view.tour === 'off') guide.endTour();
			else guide.tour = Math.max(0, Math.min(3, Number(view.tour) || 0));
		}
		if ('help' in view) {
			if (view.help === 'off') guide.closeHelp();
			else guide.openHelp(view.help);
		}
		if (view.tip) this.tip(view.tip as TipId, true);
		if (view.tip === 'off') guide.dismissTip();
		if (view.suggest === 'standard' || view.suggest === 'pro') guide.forced = view.suggest;
		else if (view.suggest === 'off') guide.forced = null;
		if (view.fail === 'off') {
			for (const [id, entry] of Object.entries(this.installs)) if (entry.state === 'failed') delete this.installs[id];
		} else if (view.fail) {
			const pkg = this.packages.find((p) => p.id === view.fail);
			if (pkg) this.installs[pkg.id] = { state: 'failed', variant: 'variant' in pkg.recommendation ? pkg.recommendation.variant : pkg.variants[0].id, done: 0, total: 0, error: `not enough disk space: ${pkg.variants[0].disk_mb ?? 384} MB needed, 120 MB free`, job: '' };
		}
		if (view.pane === 'open' || view.pane === 'closed') {
			this.pane = view.pane === 'open';
		}
		if (view.checklist === 'reset') {
			guide.counts = {};
			guide.checklistHidden = false;
			remembered.remove('checklist.hidden');
			remembered.remove('counts');
			guide.setChecklistOpen(true);
		}
		if (view.tips === 'reset') remembered.remove('tips.seen');
		if (view.tourdone === 'reset') {
			guide.tourDone = false;
			remembered.remove('tour.done');
		}
		// The window's size, for screenshots at a known size: `size=900x650`.
		const sized = view.size?.match(/^(\d{3,4})x(\d{3,4})$/);
		if (sized && api.isTauri) {
			import('@tauri-apps/api/window').then(({ getCurrentWindow, LogicalSize }) => getCurrentWindow().setSize(new LogicalSize(Number(sized[1]), Number(sized[2])))).catch(() => {});
		}
		if (view.model) this.showModel(view.model);
		if (view.chat) {
			import('./chat.svelte').then(({ chat }) => (view.chat === 'new' ? chat.newChat() : chat.open(view.chat)));
		}
	}

	/** Development only: view requests that also work before the app is entered. */
	#early(view: Record<string, string>) {
		if (view.reload) {
			location.reload();
			return;
		}
		if (this.phase !== 'ready') {
			// The first-launch flow has no #view yet: level and appearance work here too.
			if (view.level === 'guided' || view.level === 'standard' || view.level === 'pro') ux.setLevel(view.level);
			if (view.theme === 'light' || view.theme === 'dark' || view.theme === 'system') this.setTheme(view.theme);
		}
		if (view.legal === 'privacy' || view.legal === 'terms') guide.openLegal(view.legal, 'dev');
		else if (view.legal === 'off') guide.legal = null;
		// A path in this window, e.g. `/?welcome=scan` to look at a first-launch step without accepting.
		if (view.go?.startsWith('/')) {
			location.assign(view.go);
			return;
		}
		if (view.look && this.phase === 'welcome') {
			this.#looking = true;
			// Kept for this window only, so a reload after an edit stays in.
			try {
				sessionStorage.setItem('rigbrew.look', '1');
			} catch {
				// not kept
			}
			this.enter();
		}
	}

	/** One-time tips that belong to the app as a whole (the chat says its own). */
	tip(id: TipId, replay = false) {
		const text: Partial<Record<TipId, { text: string; help?: string }>> = {
			offline: { text: `This ${machine} is offline. Everything still runs here; usage data and update checks wait until it is back online.`, help: 'data' },
			service: { text: 'Rigbrew’s background service stopped and is being started again. Your chats and models are safe; downloads continue where they stopped.' },
			queued: { text: 'Rigbrew keeps a list of what you do and type here, and sends it as usage data. You can read the list, and stop it, in Privacy and data.', help: 'data' },
			nofit: { text: `Some models need more memory than this ${machine} can give them. They are greyed out and cannot be installed.`, help: 'fits' },
			switch: { text: 'Moving Pictures into memory. Only one model fits in memory at a time; switching takes a few seconds.', help: 'one-model' },
			install: { text: `Downloading. Models are kept in Rigbrew’s folder on this ${machine}; you can keep using Rigbrew meanwhile.`, help: 'disk' },
			picture: { text: 'This picture is saved with the chat. Save keeps a copy where you choose; Try again makes a new one from the same words.', help: 'pictures' },
			toolpicture: { text: 'Chat asked Pictures to make this picture. Picture mode, in the box below, goes straight there and is quicker.', help: 'pictures' }
		};
		const said = text[id];
		if (!said) return;
		if (replay) guide.replayTip({ id, ...said });
		else guide.show({ id, ...said });
	}

	go(screen: Screen, force = false) {
		this.sheet = null;
		this.palette = false;
		if (this.screen === screen && !force) return;
		this.screen = screen;
		if (screen === 'models' || screen === 'profile' || screen === 'device') guide.count(`screen.${screen}`);
		this.track('screen_viewed', { screen });
	}

	/** Opens Models on one package: scrolled to, with its details open. */
	showModel(packageId: string) {
		this.spotlight = packageId;
		this.go('models');
	}

	openSettings() {
		this.palette = false;
		if (this.sheet === 'settings') return;
		this.sheet = 'settings';
		this.track('screen_viewed', { screen: 'settings' });
	}

	/** The privacy view lives inside Settings, behind one quiet line. */
	openPrivacy() {
		this.palette = false;
		this.sheet = 'privacy';
		this.track('screen_viewed', { screen: 'privacy' });
	}

	closeSheet() {
		this.sheet = null;
	}

	togglePane() {
		this.pane = !this.pane;
		remembered.set('pane', this.pane ? 'open' : 'closed');
	}

	setTheme(theme: Theme) {
		this.theme = theme;
		remembered.set('theme', theme);
		this.applyTheme();
	}

	/**
	 * The appearance on screen. Studio is a dark workstation: while it is shown the whole window
	 * is dark (the sidebar, sheets, menus and the window frame), and the person's own choice is
	 * back when they leave it.
	 */
	shownTheme = $derived<Theme>(this.phase === 'ready' && this.screen === 'studio' ? 'dark' : this.theme);

	/** Puts the appearance on the document and, in the app, on the window frame. */
	applyTheme() {
		const theme = this.shownTheme;
		const root = document.documentElement;
		if (theme === 'system') delete root.dataset.theme;
		else root.dataset.theme = theme;
		if (api.isTauri) {
			import('@tauri-apps/api/window')
				.then(({ getCurrentWindow }) => getCurrentWindow().setTheme(theme === 'system' ? null : theme))
				.catch(() => {});
		}
	}

	/** Queues a usage event: short labels and numbers only, never what the person typed. */
	track(kind: string, payload: Payload = {}) {
		send(kind, payload);
	}

	async accept() {
		this.consent = await api.acceptConsent(this.requiredConsent);
	}

	/** The account step of the first launch comes before the app proper is entered: start the account for it. */
	wakeAccount() {
		startAccount();
	}

	async withdraw() {
		await api.withdrawConsent();
		await pauseAccount();
		this.consent = null;
		this.sheet = null;
		this.phase = 'welcome';
	}

	async refreshPackages() {
		this.packages = await api.packages();
	}

	/** Free memory and disk change while the app runs. */
	async refreshSystem() {
		try {
			this.system = await api.system();
		} catch {
			// keep the last reading
		}
	}

	async refreshUsage() {
		try {
			this.usage = await api.usage();
		} catch {
			// keep the last reading
		}
		if ((this.usage?.sync.pending ?? 0) > 0 && this.phase === 'ready') this.tip('queued');
	}

	async refreshActivity() {
		try {
			this.activity = await api.activity(371);
		} catch {
			// keep the last reading
		}
	}

	/** Asks the service to send the queued events now. Returns what went wrong, or ''. */
	async sync(): Promise<string> {
		try {
			const answer = await api.syncNow();
			await this.refreshUsage();
			return answer?.error ? answer.error : '';
		} catch (e) {
			return say(e);
		}
	}

	// --- updates and sponsors ------------------------------------------------------------------

	/** Asks for the app config: on start, every few hours, and from Settings. */
	async loadConfig() {
		this.checkingConfig = true;
		try {
			const config = pretendConfig() ?? (await api.appConfig(APP_VERSION));
			this.config = config;
			this.configCheckedAt = Date.now();
		} catch {
			return; // the service is away; keep what is known
		} finally {
			this.checkingConfig = false;
		}
		const version = this.config?.latest_version ?? 'next';
		if (this.update !== 'none' && (this.update === 'hard' || this.updateNotice) && this.#shownUpdate !== `${this.update}:${version}`) {
			this.#shownUpdate = `${this.update}:${version}`;
			this.track('update_shown', { update: this.update, from: APP_VERSION, to: version });
		}
		// The consent text changed on the server: ask again.
		if (this.phase === 'ready' && !this.consented && !this.#looking) this.phase = 'welcome';
	}

	/** Opens the download page. The in-app updater comes later. */
	acceptUpdate() {
		this.track('update_accepted', { update: this.update, from: APP_VERSION, to: this.config?.latest_version });
		if (this.config?.download_url) api.openExternal(this.config.download_url);
	}

	dismissUpdate() {
		this.dismissedUpdate = this.config?.latest_version ?? 'next';
		remembered.set('update.dismissed', this.dismissedUpdate);
		this.track('update_dismissed', { from: APP_VERSION, to: this.config?.latest_version });
	}

	openSponsor(where: string, url?: string, tier?: string) {
		const target = url ?? this.sponsors?.url;
		if (!target) return;
		this.track('sponsor_opened', { where, tier });
		api.openExternal(target);
	}

	// --- downloads -------------------------------------------------------------------------------

	/** Downloads a build of a package (the recommended one unless named) and follows the job. */
	/** `acceptTerms`: the person agreed to the package's licence terms just now (asked by the install button). */
	async install(pkg: Package, variant?: string, acceptTerms = false) {
		const rec = pkg.recommendation;
		const id = variant ?? (rec.verdict === 'run' ? rec.variant : '');
		const build = pkg.variants.find((v) => v.id === id);
		if (!build || this.installs[pkg.id]?.state === 'running') return;
		this.installs[pkg.id] = { state: 'running', variant: id, done: 0, total: build.disk_mb * 1024 * 1024, error: '', job: '' };
		const entry = this.installs[pkg.id];
		guide.show({
			id: 'install',
			text: `Downloading ${nameOf(pkg, ux.flags)}: ${size(build.disk_mb)}. It is kept in Rigbrew’s folder on this ${machine}${this.system ? ` (${this.system.storage.data_dir.replace(/^\/Users\/[^/]+/, '~')})` : ''}. You can keep using Rigbrew meanwhile.`,
			help: 'disk'
		});
		try {
			entry.job = await api.pull(pkg.id, id, acceptTerms);
			this.#follow(pkg.id, entry.job);
		} catch (e) {
			entry.state = 'failed';
			entry.error = say(e);
		}
	}

	/** Stops a running download. What is already on disk stays, so installing again resumes. */
	async cancelInstall(packageId: string) {
		const entry = this.installs[packageId];
		if (!entry?.job) return;
		try {
			await api.cancelJob(entry.job);
		} catch (e) {
			// 404: it ended by itself in the meantime.
			if (!(e instanceof api.ApiError && e.status === 404)) throw e;
		}
		delete this.installs[packageId];
		this.refreshPackages().catch(() => {});
	}

	/** Picks up downloads the service is running: after a reload, an app restart or a service restart. */
	async attachJobs() {
		let list: Awaited<ReturnType<typeof api.jobs>>;
		try {
			list = await api.jobs();
		} catch {
			return;
		}
		for (const { id, job } of list) {
			if (job.state !== 'running') continue;
			const current = this.installs[job.package];
			if (current?.state === 'running' && (current.job === id || current.job === '')) continue;
			this.installs[job.package] = { state: 'running', variant: job.variant, done: job.done, total: job.total, error: '', job: id };
			this.#follow(job.package, id);
		}
		// A download this app believes is running but the service no longer knows (it was restarted).
		let dropped = false;
		for (const [packageId, entry] of Object.entries(this.installs)) {
			if (entry.state === 'running' && entry.job && !list.some((j) => j.id === entry.job)) {
				delete this.installs[packageId];
				dropped = true;
			}
		}
		if (dropped) this.refreshPackages().catch(() => {});
	}

	async #follow(packageId: string, jobId: string) {
		for (;;) {
			const entry = this.installs[packageId];
			if (!entry || entry.job !== jobId || entry.state !== 'running') return; // cancelled or replaced
			let job;
			try {
				job = await api.job(jobId);
			} catch (e) {
				if (e instanceof api.ApiError && e.status === 404) return; // the service restarted; attachJobs takes over
				await pause(1500);
				continue;
			}
			entry.done = job.done;
			entry.total = job.total || entry.total;
			if (job.state === 'done') {
				await this.refreshPackages().catch(() => {});
				if (this.installs[packageId]?.job === jobId) delete this.installs[packageId];
				this.refreshSystem();
				this.refreshUsage();
				return;
			}
			if (job.state === 'failed') {
				entry.state = 'failed';
				entry.error = job.error ?? 'The download failed.';
				return;
			}
			if (job.state === 'cancelled') {
				if (this.installs[packageId]?.job === jobId) delete this.installs[packageId];
				return;
			}
			await pause(400);
		}
	}

	/** Adds a community model (one file of a repo that was looked up) and follows its download. Returns its package id. */
	async addCommunity(repo: string, file: string, bytes: number): Promise<string> {
		const added = await api.communityAdd(repo, file);
		this.installs[added.id] = { state: 'running', variant: 'gguf', done: 0, total: bytes, error: '', job: added.job };
		await this.refreshPackages().catch(() => {});
		this.#follow(added.id, added.job);
		return added.id;
	}

	/** Removes a community model: its files and its place in the list. Returns how many bytes that freed. */
	async removeCommunity(id: string): Promise<number> {
		const answer = await api.communityRemove(id);
		delete this.installs[id];
		await Promise.all([this.refreshPackages().catch(() => {}), this.refreshSystem(), this.refreshUsage()]);
		import('./chat.svelte').then(({ chat }) => chat.syncEngine());
		return answer.freed_bytes;
	}

	/** Removes one installed build, or all of a package. Returns how many bytes that freed. */
	async remove(pkg: Package, variant?: string): Promise<number> {
		const answer = await api.removePackage(pkg.id, variant);
		await Promise.all([this.refreshPackages().catch(() => {}), this.refreshSystem(), this.refreshUsage()]);
		// The service unloads a model whose files are removed.
		import('./chat.svelte').then(({ chat }) => chat.syncEngine());
		return answer.freed_bytes;
	}
}

export const app = new AppState();
