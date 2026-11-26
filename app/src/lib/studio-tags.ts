// Help tags: `use:tag={'play'}` on a control gives it the tag of that tool (see studio-tools.ts),
// `use:tag={{ name, what, key }}` one of its own words. The one StudioTags component shows them.
// A Studio control that has only a title is shown with the same tag, made from its title, so
// every control speaks the same way and none falls back to the system's slow tooltip.

import { tagFromTitle, tagOf, type Tag, type TagSpec } from './studio-tools.ts';

const tagged = new WeakMap<Element, Tag>();

export function tag(node: HTMLElement, spec: TagSpec) {
	const set = (next: TagSpec) => {
		const made = tagOf(next);
		if (made) tagged.set(node, made);
		else tagged.delete(node);
		node.removeAttribute('title');
	};
	set(spec);
	return {
		update: set,
		destroy: () => tagged.delete(node)
	};
}

/**
 * A control that cannot be used now, under a point. Not every webview sends a disabled control
 * pointer events, so the Studio lets the pointer go through them, and one is looked for among the
 * controls of the element under the pointer. Its tag says why it cannot be used.
 */
export function disabledAt(target: EventTarget | null, x: number, y: number, root: Element): { el: HTMLElement; tag: Tag } | null {
	if (!(target instanceof Element) || target === root || !root.contains(target)) return null;
	for (const el of target.querySelectorAll<HTMLElement>('button:disabled, input:disabled, select:disabled')) {
		const r = el.getBoundingClientRect();
		if (x < r.left || x > r.right || y < r.top || y > r.bottom) continue;
		const found = tagAt(el, root);
		if (found) return found;
	}
	return null;
}

/** The tagged control at or around an element inside `root`, and its tag. */
export function tagAt(target: EventTarget | null, root: Element): { el: HTMLElement; tag: Tag } | null {
	let el = target instanceof Element ? target : null;
	while (el && root.contains(el)) {
		const own = tagged.get(el);
		if (own && el instanceof HTMLElement) return { el, tag: own };
		if (el instanceof HTMLElement && (el.title || el.dataset.tip)) {
			if (el.title) {
				// The system's own tooltip would show the same words a moment later.
				if (!el.getAttribute('aria-label') && !el.textContent?.trim()) el.setAttribute('aria-label', el.title);
				el.dataset.tip = el.title;
				el.removeAttribute('title');
			}
			const made = tagFromTitle(el.dataset.tip ?? '', el.getAttribute('aria-label') ?? el.textContent ?? '');
			if (made) return { el, tag: made };
		}
		el = el.parentElement;
	}
	return null;
}
