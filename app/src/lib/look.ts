// The app lends the service its browser. A Code task that wants to see a page it made asks
// the service; the service has no browser, so the page is opened here, in a hidden window of
// the app (src-tauri/src/look.rs), and what was seen goes back. This file is the go-between:
// it waits at the service for a page to open, hands it to the native side, and returns the answer.

import * as api from './api';

const wait = (ms: number) => new Promise((r) => setTimeout(r, ms));

let watching = false;

/** Starts taking looks, once, for as long as the window is open. Does nothing outside the app. */
export async function watchLooks() {
	if (watching || !api.isTauri) return;
	watching = true;
	const { invoke } = await import('@tauri-apps/api/core');
	for (;;) {
		try {
			const job = await api.nextLook();
			if (job === undefined) {
				// A service too old to ask. A newer one may take its place.
				await wait(60_000);
				continue;
			}
			if (!job) continue;
			let seen: unknown;
			try {
				seen = await invoke('look_page', { url: job.url, width: job.width, height: job.height, actions: job.actions });
			} catch (e) {
				seen = { url: job.url, note: `The page could not be opened: ${e instanceof Error ? e.message : String(e)}` };
			}
			await api.answerLook(job.id, seen).catch(() => {});
		} catch {
			// The service is away for a moment.
			await wait(3000);
		}
	}
}
