// Shapes returned by rigbrewd. The contract is daemon/src/main.rs and docs/api.md.

export interface System {
	tier: string;
	cpu: { brand: string; physical_cores: number; logical_cores: number; features: string[] };
	ram_total_mb: number;
	ram_available_mb: number;
	gpus: {
		name: string;
		vendor: string;
		backend: string;
		kind: string;
		memory_total_mb: number | null;
		memory_free_mb: number | null;
	}[];
	os: { family: string; version: string; arch: string };
	storage: { data_dir: string; free_mb: number };
	accel_budget_mb: number;
	cpu_budget_mb: number;
}

export type Metrics = Record<string, number | string | boolean | null>;

export interface Variant {
	id: string;
	quality: number;
	disk_mb: number;
	accel_mem_mb: number;
	metrics: Metrics;
}

export type Recommendation =
	| {
			verdict: 'run';
			variant: string;
			placement: 'accelerator' | 'cpu';
			compromise: boolean;
			memory_mb: number;
			disk_mb: number;
			reason: string;
	  }
	| { verdict: 'needs_disk'; variant: string; needed_mb: number; free_mb: number }
	| { verdict: 'too_heavy'; smallest_mb: number; budget_mb: number; reason: string };

export interface Package {
	id: string;
	name: string;
	modality: string;
	source: { name: string; url: string; license: string; changes: string };
	variants: Variant[];
	recommendation: Recommendation;
	installed: string[];
	/** Only signed-in accounts may download it (the server's app config decides). */
	account_required?: boolean;
	/** How to ask the package for its work, from the manifest (video: steps, sizes, lengths, sound). */
	defaults?: {
		steps?: number;
		frames?: number;
		audio?: boolean;
		sizes?: Record<string, [number, number]>;
		lengths?: { frames: number; label: string }[];
	} | null;
	/** A notice the source licence wants shown wherever the package is used. */
	credit?: string | null;
	/** Licence terms to agree to before the download. */
	terms?: { summary: string; url: string } | null;
	/** Added by the person from a link (Hugging Face or GitHub). Not in Rigbrew's catalogue and not tested by Rigbrew. */
	community?: boolean;
}

/** One GGUF file a looked-up repo offers. */
export interface CommunityFile {
	file: string;
	/** Bytes. */
	size: number;
	/** Memory it needs: its size and about 15% more. */
	needs_mb: number;
	fits: boolean;
	/** The site gives a sha256, so the download is verified. */
	checked: boolean;
	/** The package id it has, or would get. */
	id: string;
	installed: boolean;
}

/** `POST /v1/community/lookup`: what a pasted link holds. */
export interface CommunityFound {
	host: 'huggingface' | 'github';
	/** "Hugging Face" or "GitHub". */
	site: string;
	owner: string;
	repo: string;
	url: string;
	license: string | null;
	/** The owner asks for a sign-in before downloading; Rigbrew cannot install it. */
	gated: boolean;
	budget_mb: number;
	files: CommunityFile[];
	/** The file to preselect: the one the link named, or the largest that fits. */
	pick: string | null;
	/** GGUF files left out (parts of split models, picture add-ons). */
	skipped: number;
	/** Hugging Face models a GitHub README links to, when the repo has no files itself. */
	links: string[];
}

export interface Job {
	package: string;
	variant: string;
	state: 'running' | 'done' | 'failed' | 'cancelled';
	file: string;
	done: number;
	total: number;
	error: string | null;
}

export interface Consent {
	version: string;
	accepted_at_ms: number;
}

export interface QueuedEvent {
	id: string;
	ts_ms: number;
	kind: string;
	install_id: string;
	session_id: string;
	user_id: string | null;
	payload: unknown;
	/** True when the event carries what the person wrote (prompts, answers): sent only with consent. */
	content?: boolean;
}

/** What the app tells the service about who is signed in (`PUT /v1/session`). */
export interface Session {
	uid: string;
	id_token: string;
	expires_at_ms: number;
	anonymous: boolean;
	email: string | null;
	name: string | null;
}

export interface UsageTotals {
	chat_messages: number;
	tokens_in: number;
	tokens_out: number;
	images: number;
	seconds_generating: number;
}

export interface Usage {
	today: UsageTotals;
	last_7_days: UsageTotals;
	all_time: UsageTotals;
	disk: { packages: { id: string; name?: string; variant: string; bytes: number }[]; total_bytes: number; free_bytes: number };
	sync: { pending: number; last_sent_ms: number | null; last_error?: string | null };
}

export interface SponsorTier {
	name: string;
	amount: string | number;
	url: string;
}

/** The server's app config, as the service passes it on. Every field may be missing. */
export interface AppConfig {
	latest_version?: string;
	min_version?: string;
	update?: 'none' | 'soft' | 'hard';
	download_url?: string;
	notes?: string;
	consent_version?: string;
	manifest_url?: string;
	sponsors?: { url?: string; headline?: string; tiers?: SponsorTier[] };
	message?: string;
	/** True when the server could not be asked and this is the last answer kept on disk. */
	stale?: boolean;
}

export interface ActivityDay {
	/** Days since the Unix epoch, UTC. */
	day: number;
	prompts: number;
	images: number;
	tokens_out: number;
	events: number;
}

/** `GET /v1/activity`: what the profile page draws. */
export interface Activity {
	today: number;
	days: ActivityDay[];
	packages: { id: string; name: string; prompts: number }[];
	first_event_ms: number | null;
	install_id: string;
	all_time: UsageTotals;
}
