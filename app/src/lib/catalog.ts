// The community catalog: models Rigbrew suggests, read by the service from a signed file
// (`GET /v1/community/catalog`). Here they are put in groups, one per kind, in a fixed order.

import type { CatalogModel } from './types';

/** The kinds the catalog knows, in the order they are listed, with their plain names. */
export const KINDS: [kind: string, name: string][] = [
	['chat', 'Chat'],
	['image', 'Pictures'],
	['video', 'Video'],
	['speech', 'Speech'],
	['listen', 'Listening'],
	['music', 'Music'],
	['tool', 'Tools']
];

export interface CatalogGroup {
	kind: string;
	name: string;
	models: CatalogModel[];
}

/** The models by kind. A kind this build has not heard of goes under "Other", last; empty groups are left out. */
export function groups(models: CatalogModel[]): CatalogGroup[] {
	const known = new Set(KINDS.map(([kind]) => kind));
	const all = [
		...KINDS.map(([kind, name]) => ({ kind, name, models: models.filter((m) => m.kind === kind) })),
		{ kind: 'other', name: 'Other', models: models.filter((m) => !known.has(m.kind)) }
	];
	return all.filter((g) => g.models.length > 0);
}

/** "Hugging Face" or "GitHub", for a link the service let through. */
export const siteOf = (url: string) => (url.startsWith('https://github.com/') ? 'GitHub' : 'Hugging Face');
