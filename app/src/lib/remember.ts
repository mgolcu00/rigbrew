// Small per-install preferences in localStorage. Storage may be unavailable (a private window,
// blocked site data, a webview that refuses it): every read falls back to a default and every
// write is allowed to fail, so the app works the same without it, it just forgets.

const key = (name: string) => `rigbrew.${name}`;

export const remembered = {
	get(name: string): string | null {
		try {
			return localStorage.getItem(key(name));
		} catch {
			return null;
		}
	},
	set(name: string, value: string) {
		try {
			localStorage.setItem(key(name), value);
		} catch {
			// not remembered
		}
	},
	remove(name: string) {
		try {
			localStorage.removeItem(key(name));
		} catch {
			// nothing to forget
		}
	},
	/** A JSON value, or `fallback` when it is missing or unreadable. */
	json<T>(name: string, fallback: T): T {
		const raw = this.get(name);
		if (raw === null) return fallback;
		try {
			return JSON.parse(raw) as T;
		} catch {
			return fallback;
		}
	},
	setJson(name: string, value: unknown) {
		this.set(name, JSON.stringify(value));
	}
};
