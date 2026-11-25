// The account, switched off. Same exports as `src/account/index.ts`, and nothing behind them:
// no Firebase, no server calls, no sign-in. This is what a build uses unless
// `RIGBREW_ACCOUNT=1` is set (see `vite.config.ts`). The app then reports "no account", the
// profile shows local activity only, and usage events carry no account id.

import type { AccountView } from './types';

export { default as AccountPanel } from './AccountPanel.svelte';
export { default as AccountDataLine } from './AccountDataLine.svelte';
export { default as AccountWelcome } from './AccountWelcome.svelte';
export type { AccountUser, AccountView } from './types';

/** Whether this build has accounts at all. */
export const ACCOUNTS = false;

const nobody: AccountView = { user: null, label: 'This install', detail: 'no account in this build' };

export const getAccount = (): AccountView => nobody;

/** Calls `listener` once: nothing ever changes here. */
export function onAccountChange(listener: (view: AccountView) => void): () => void {
	listener(nobody);
	return () => {};
}

export const startAccount = async (): Promise<void> => {};
export const pauseAccount = async (): Promise<void> => {};
export const pushSession = async (): Promise<void> => {};
