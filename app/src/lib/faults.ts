// Errors the interface did not catch: a script error or a promise nobody handled. Each is sent as
// a `ui_error` usage event with a short message, the script's file name and the line, so a broken
// screen shows up in the error report. No text from the page goes with it, and messages are cut
// to a length that passes as basic usage data. A few per session, each distinct one once.

import { track } from './track';

const LIMIT = 8;
const seen = new Set<string>();

const cut = (text: unknown, max = 110) => String(text ?? '').replace(/\s+/g, ' ').slice(0, max);
/** "http://localhost:1420/_app/immutable/chunks/abc.js" -> "abc.js": the path says nothing more. */
const file = (url: unknown) => String(url ?? '').split(/[?#]/)[0].split('/').pop() ?? '';

function report(kind: 'error' | 'rejection', message: string, source: string, line: number, column: number, stack: string) {
	const key = `${kind}|${message}|${source}|${line}`;
	if (seen.has(key) || seen.size >= LIMIT) return;
	seen.add(key);
	// The first frame of the stack names the function; later frames are mostly the framework.
	const frame = stack.split('\n').map((l) => l.trim()).find((l) => l && !l.startsWith(message.slice(0, 20))) ?? '';
	track('ui_error', { where: kind, message: cut(message), source: cut(source, 60), line, column, frame: cut(frame.replace(/https?:\/\/[^/]+\//g, '').replace(/\?[^:)]*/g, ''), 110), screen: location.hash || location.pathname });
}

/** Starts listening. Returns how to stop. */
export function watchFaults(): () => void {
	const onError = (e: ErrorEvent) => report('error', e.message || 'Script error', file(e.filename), e.lineno ?? 0, e.colno ?? 0, e.error instanceof Error ? (e.error.stack ?? '') : '');
	const onRejection = (e: PromiseRejectionEvent) => {
		const r = e.reason;
		report('rejection', r instanceof Error ? `${r.name}: ${r.message}` : cut(r), '', 0, 0, r instanceof Error ? (r.stack ?? '') : '');
	};
	window.addEventListener('error', onError);
	window.addEventListener('unhandledrejection', onRejection);
	return () => {
		window.removeEventListener('error', onError);
		window.removeEventListener('unhandledrejection', onRejection);
	};
}
