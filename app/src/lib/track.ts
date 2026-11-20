// Usage events (`POST /v1/events`). Kinds are 1 to 48 of a-z, 0-9 and _; payloads carry short
// labels and numbers only, never what the person typed. The service decides what may be kept
// (basic usage data always, content only with consent) and drops the rest.

import { recordEvent } from './api';

export type Payload = Record<string, string | number | boolean | null | undefined>;

export function track(kind: string, payload: Payload = {}) {
	recordEvent(kind, payload).catch(() => {});
}
