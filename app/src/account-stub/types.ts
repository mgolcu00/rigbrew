// The shape the rest of the app sees. `src/account/types.ts` is the same file.

export interface AccountUser {
	uid: string;
	anonymous: boolean;
	email: string | null;
	name: string | null;
}

export interface AccountView {
	/** Who is signed in on this install, anonymous accounts included; null when nobody is. */
	user: AccountUser | null;
	/** What to call them in one or two words: a name, an email, "Anonymous", or "This install". */
	label: string;
	/** One short line under the label: "anonymous account", an email, "works without one". */
	detail: string;
}
