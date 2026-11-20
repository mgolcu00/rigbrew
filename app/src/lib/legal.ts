// The legal texts, single-sourced from docs/legal in the repository and bundled at build time
// (vite.config.ts allows that folder). Shown inside the app by components/LegalSheet.svelte.

import privacy from '../../../docs/legal/privacy.md?raw';
import terms from '../../../docs/legal/terms.md?raw';

export type LegalDoc = 'privacy' | 'terms';

export const legal: Record<LegalDoc, { title: string; text: string; file: string }> = {
	privacy: { title: 'Privacy Policy', text: privacy, file: 'privacy.md' },
	terms: { title: 'Terms of Use', text: terms, file: 'terms.md' }
};

/** A link between the two texts (`privacy.md`, `terms.md`), or null for anything else. */
export function legalLink(href: string): LegalDoc | null {
	const name = href.split(/[?#]/)[0].split('/').pop() ?? '';
	if (name === 'privacy.md') return 'privacy';
	if (name === 'terms.md') return 'terms';
	return null;
}
