// Guidance: the Help sheet, the first tour, one-time tips, the counters behind the
// getting-started checklist, and when to suggest the next level. Everything here is remembered
// per install (remember.ts) and works without storage, it just forgets.

import { LEVELS, ux, type Level } from './experience.svelte';
import { remembered } from './remember';
import { track } from './track';
import type { Package, Usage } from './types';
import { mod } from './words';

export type ChecklistId = 'ask' | 'picture' | 'models' | 'profile' | 'search';

export type TipId = 'switch' | 'install' | 'nofit' | 'picture' | 'toolpicture' | 'queued' | 'offline' | 'service';

export interface Tip {
	id: TipId;
	text: string;
	/** A help article that says more. */
	help?: string;
}

/** Things counted to tick the checklist and to judge readiness. Names are stable keys. */
export type Counter = 'screen.models' | 'screen.profile' | 'screen.device' | 'palette' | 'slash' | 'asked' | 'pictures' | 'help';

const WEEK = 7 * 24 * 60 * 60 * 1000;

export const TOUR_STEPS = 4;

class Guide {
	/** The Help sheet and the article it shows ('' is the list). */
	helpOpen = $state(false);
	article = $state('');

	/** The legal text open in its sheet, or null. */
	legal = $state<'privacy' | 'terms' | null>(null);

	openLegal(doc: 'privacy' | 'terms', from: string) {
		this.legal = doc;
		track('legal_opened', { doc, from });
	}

	/** The tour step on screen, or null. */
	tour = $state<number | null>(null);
	tourDone = $state(remembered.get('tour.done') === '1');

	/** The tip on screen; more wait their turn. */
	tip = $state<Tip | null>(null);
	#waiting: Tip[] = [];
	#seen = new Set<string>(remembered.json<string[]>('tips.seen', []));

	counts = $state<Partial<Record<Counter, number>>>(remembered.json('counts', {}));
	checklistOpen = $state(remembered.get('checklist.open') !== 'closed');
	/** Put away for good, after "You know the basics". */
	checklistHidden = $state(remembered.get('checklist.hidden') === '1');

	hideChecklist() {
		this.checklistHidden = true;
		remembered.set('checklist.hidden', '1');
	}

	/** "Not now" on a suggestion keeps it away for a week. */
	snoozed = $state<Partial<Record<Level, number>>>(remembered.json('suggest.snoozed', {}));
	/** Development only: show a suggestion now, whatever the counts say. */
	forced = $state<Level | null>(null);

	// --- help -------------------------------------------------------------------------------------

	/** `from`: where it was opened (titlebar, shortcut, hint, tip, palette, settings, error, checklist). */
	openHelp(article = '', from = 'titlebar') {
		this.article = article;
		if (!this.helpOpen) track('help_opened', { from });
		this.helpOpen = true;
		if (article) track('help_article_opened', { article, from });
		this.count('help');
	}

	/** An article picked inside the sheet: from the list, or the first search result. */
	showArticle(article: string, from: 'list' | 'search') {
		this.article = article;
		track('help_article_opened', { article, from });
	}

	closeHelp() {
		this.helpOpen = false;
	}

	// --- the tour -----------------------------------------------------------------------------------

	/** `from`: first_run, help, palette, settings. */
	startTour(from = 'help') {
		this.helpOpen = false;
		this.tip = null;
		this.tour = 0;
		track('tour_started', { from });
		track('tour_step', { step: 1 });
	}

	/** Shown once, after the first-launch flow. */
	startTourOnce() {
		if (this.tourDone || this.tour !== null) return;
		// After the first screen has drawn, so the marks find their targets.
		setTimeout(() => this.startTour('first_run'), 700);
	}

	nextStep() {
		if (this.tour === null) return;
		if (this.tour + 1 >= TOUR_STEPS) this.endTour(true);
		else {
			this.tour++;
			track('tour_step', { step: this.tour + 1 });
		}
	}

	previousStep() {
		if (this.tour !== null && this.tour > 0) {
			this.tour--;
			track('tour_step', { step: this.tour + 1, back: true });
		}
	}

	/** Finished on the last step, or skipped (Esc, "Skip tour") on any. */
	endTour(finished = false) {
		if (this.tour !== null) {
			if (finished) track('tour_finished');
			else track('tour_skipped', { step: this.tour + 1 });
		}
		this.tour = null;
		this.tourDone = true;
		remembered.set('tour.done', '1');
		this.#next();
	}

	// --- tips -----------------------------------------------------------------------------------------

	/** Shows a tip the first time something happens; never again after. Pro shows none. */
	show(tip: Tip) {
		if (ux.flags.tips === 'none' || this.#seen.has(tip.id)) return;
		this.#seen.add(tip.id);
		remembered.setJson('tips.seen', [...this.#seen]);
		if (this.tip || this.tour !== null) this.#waiting.push(tip);
		else this.#put(tip);
	}

	#put(tip: Tip) {
		this.tip = tip;
		track('tip_shown', { tip: tip.id });
	}

	/** `how`: got_it, more (opened the help article). */
	dismissTip(how: 'got_it' | 'more' = 'got_it') {
		if (this.tip) track('tip_dismissed', { tip: this.tip.id, how });
		this.tip = null;
		this.#next();
	}

	#next() {
		if (this.tour !== null || this.tip) return;
		const next = this.#waiting.shift();
		if (next) setTimeout(() => this.#put(next), 250);
	}

	/** Development only: forget which tips were seen and show one. */
	replayTip(tip: Tip) {
		this.#seen.delete(tip.id);
		this.tip = null;
		this.show(tip);
	}

	// --- counters and the checklist --------------------------------------------------------------------

	count(name: Counter) {
		this.counts[name] = (this.counts[name] ?? 0) + 1;
		remembered.setJson('counts', this.counts);
	}

	setChecklistOpen(open: boolean) {
		this.checklistOpen = open;
		remembered.set('checklist.open', open ? 'open' : 'closed');
	}

	/** The getting-started steps, each ticked by what was really done. */
	checklist(usage: Usage | null, pictures: boolean): { id: ChecklistId; label: string; done: boolean }[] {
		const c = this.counts;
		const items: { id: ChecklistId; label: string; done: boolean }[] = [
			{ id: 'ask', label: 'Ask a question', done: (usage?.all_time.chat_messages ?? 0) > 0 || (c.asked ?? 0) > 0 },
			{ id: 'picture', label: 'Make a picture', done: (usage?.all_time.images ?? 0) > 0 || (c.pictures ?? 0) > 0 },
			{ id: 'models', label: 'Look at your models', done: (c['screen.models'] ?? 0) > 0 },
			{ id: 'profile', label: 'Open your profile', done: (c['screen.profile'] ?? 0) > 0 },
			{ id: 'search', label: `Find anything with ${mod}K`, done: (c.palette ?? 0) > 0 }
		];
		return pictures ? items : items.filter((i) => i.id !== 'picture');
	}

	// --- growing with the person -------------------------------------------------------------------------

	/**
	 * The level to suggest now, or null. Based on what was really done: messages and pictures
	 * from the service's own totals, visits and searches counted here, a second version installed.
	 * Only ever upwards, and never while "Not now" holds.
	 */
	suggestion(usage: Usage | null, packages: Package[], now = Date.now()): Level | null {
		const next = ux.next;
		if (!next) return null;
		if (this.forced === next) return next;
		if ((this.snoozed[next] ?? 0) > now) return null;
		const c = this.counts;
		const messages = usage?.all_time.chat_messages ?? 0;
		const images = usage?.all_time.images ?? 0;
		if (next === 'standard') {
			if ((messages >= 10 && images >= 1) || (c['screen.models'] ?? 0) >= 2) return next;
		} else if (next === 'pro') {
			if ((c.palette ?? 0) + (c.slash ?? 0) >= 6 || packages.some((p) => p.installed.length >= 2)) return next;
		}
		return null;
	}

	snooze(level: Level) {
		this.forced = null;
		this.snoozed[level] = Date.now() + WEEK;
		remembered.setJson('suggest.snoozed', this.snoozed);
		track('level_suggestion_dismissed', { to: level });
	}

	#suggested = new Set<Level>();
	/** Counted once per run of the app for each level suggested. */
	suggestedOnce(level: Level) {
		if (this.#suggested.has(level)) return;
		this.#suggested.add(level);
		track('level_suggested', { to: level });
	}

	/** Checklist items reported as done, so each is counted once. */
	#reported = new Set<string>(remembered.json<string[]>('checklist.reported', []));
	reportChecklist(done: string[], total: number) {
		let changed = false;
		for (const item of done) {
			if (this.#reported.has(item)) continue;
			this.#reported.add(item);
			changed = true;
			track('checklist_item_done', { item });
		}
		if (!changed) return;
		remembered.setJson('checklist.reported', [...this.#reported]);
		if (done.length === total) track('checklist_completed', { items: total });
	}

	/** Why the suggestion is made and what the level adds, from what was really done. */
	reason(level: Level, usage: Usage | null, packages: Package[]): string {
		const plural = (n: number, one: string, many: string) => `${n} ${n === 1 ? one : many}`;
		if (level === 'standard') {
			const messages = usage?.all_time.chat_messages ?? 0;
			const pictures = usage?.all_time.images ?? 0;
			const why =
				messages >= 10
					? `You’ve sent ${plural(messages, 'message', 'messages')}${pictures > 0 ? ` and made ${plural(pictures, 'picture', 'pictures')}` : ''}.`
					: 'You’ve looked at your models a few times.';
			return `${why} Standard also shows model versions, speeds and how memory is used.`;
		}
		const why = packages.some((p) => p.installed.length >= 2) ? 'You have two versions of a model installed.' : 'You use search and commands often.';
		return `${why} Pro also shows every build with its measurements, the raw ids and the shortcuts.`;
	}

	levelName = (level: Level) => LEVELS[level].name;
}

export const guide = new Guide();
