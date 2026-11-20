import type { Metrics, Package, System } from './types';

/** 4177 -> "4.1 GB", 384 -> "384 MB". */
export function size(mb: number): string {
	if (mb < 1000) return `${Math.round(mb)} MB`;
	const gb = mb / 1024;
	return `${gb >= 100 ? Math.round(gb) : gb.toFixed(1).replace(/\.0$/, '')} GB`;
}

/** Bytes as the service counts them: 402403328 -> "384 MB". */
export const bytes = (count: number) => size(count / 1024 / 1024);

/** 1234 -> "1,234", 1234567 -> "1.23 M". */
export function count(value: number): string {
	if (value >= 1_000_000) return `${(value / 1_000_000).toFixed(2)} M`;
	return Math.round(value).toLocaleString('en-US');
}

/** Time spent generating: 42 -> "42 s", 754 -> "12 min 34 s", 9000 -> "2 h 30 min". */
export function duration(value: number): string {
	const total = Math.round(value);
	if (total < 60) return `${total} s`;
	if (total < 3600) return `${Math.floor(total / 60)} min ${total % 60} s`;
	return `${Math.floor(total / 3600)} h ${Math.floor((total % 3600) / 60)} min`;
}

/** How long ago, to the nearest unit that matters: "just now", "2 min ago", "3 h ago", "5 days ago". */
export function ago(ms: number, now = Date.now()): string {
	const s = Math.max(0, (now - ms) / 1000);
	if (s < 45) return 'just now';
	if (s < 3600) return `${Math.max(1, Math.round(s / 60))} min ago`;
	if (s < 86400) return `${Math.round(s / 3600)} h ago`;
	const days = Math.round(s / 86400);
	return `${days} ${days === 1 ? 'day' : 'days'} ago`;
}

export const percent = (ratio: number, digits = 1) => `${(ratio * 100).toFixed(digits)}%`;

export type FamilyIcon = 'chat' | 'image' | 'video' | 'voice' | 'listening';

export interface Family {
	id: string;
	name: string;
	/** The package modality this family serves, as the service names it. */
	modality: string;
	does: string;
	icon: FamilyIcon;
	/** Whether Rigbrew can run this family today, not only download it. */
	runs: boolean;
	/** What installing it changes in the chat. */
	adds: string;
	/** What it can do, in a few words, for tables and lists. */
	can: string;
}

// Order is the order of the Library.
export const families: Family[] = [
	{ id: 'steep', name: 'Steep', modality: 'chat', does: 'Chat', icon: 'chat', runs: true, adds: 'The model you talk to. It can call the other installed families for you.', can: 'Answers, writes, explains, calls the image model' },
	{ id: 'crema', name: 'Crema', modality: 'image', does: 'Image', icon: 'image', runs: true, adds: 'Adds Image mode to the composer, and the chat model can make a picture when you ask for one.', can: 'Makes a picture from a description' },
	{ id: 'pour', name: 'Pour', modality: 'video', does: 'Video', icon: 'video', runs: true, adds: 'Adds Video mode to the composer: a short clip from a description, in a few minutes.', can: 'Short video clips from a description' },
	{ id: 'whistle', name: 'Whistle', modality: 'voice', does: 'Voice', icon: 'voice', runs: true, adds: 'Adds Read aloud to every answer in the chat. Reads English.', can: 'Reads answers aloud, in English' },
	{ id: 'sift', name: 'Sift', modality: 'listen', does: 'Listening', icon: 'listening', runs: true, adds: 'Adds a microphone to the composer: talk instead of typing, in English, Turkish and most other languages.', can: 'Writes down what you say' }
];

/** Said wherever a family can be downloaded but not run. */
export const RUNS_LATER = 'Download works; running it in Rigbrew comes later.';

/** One or two sentences on how families fit together; shown at first launch and in the Library. */
export const FAMILIES_EXPLAINED =
	'Models are grouped in families: chat, image, video, voice and listening. Each family installs separately, and the ones you install work together inside the same chat.';

export function familyOf(pkg: Package): Family {
	const prefix = pkg.id.split('-')[0];
	return (
		families.find((f) => f.id === prefix) ??
		families.find((f) => f.modality === pkg.modality) ?? { id: prefix, name: prefix, modality: pkg.modality, does: pkg.modality, icon: 'chat', runs: false, adds: '', can: '' }
	);
}

/** The build the service loads for an installed package: the best one on disk. */
export function installedVariant(pkg: Package) {
	return pkg.variants.filter((v) => pkg.installed.includes(v.id)).sort((a, b) => b.quality - a.quality)[0];
}

/** "i8" -> "int8"; other build ids are shown as they are. */
export const variantName = (id: string) => id.replace(/^i(\d+)$/, 'int$1');

/** A memory figure without its unit, for rulers: 4177 -> "4.1", 36864 -> "36". */
export const gb = (mb: number) => {
	const value = mb / 1024;
	return value >= 10 ? String(Math.round(value)) : value.toFixed(1).replace(/\.0$/, '');
};

/** How long ago, as short as a status line needs: "now", "2m", "3h", "5d". */
export function agoShort(ms: number, now = Date.now()): string {
	const s = Math.max(0, (now - ms) / 1000);
	if (s < 45) return 'now';
	if (s < 3600) return `${Math.max(1, Math.round(s / 60))}m`;
	if (s < 86400) return `${Math.round(s / 3600)}h`;
	return `${Math.round(s / 86400)}d`;
}

/** The memory budget models are placed in on this machine, and what to call it. */
export function budgetOf(system: System): { mb: number; where: 'GPU' | 'CPU'; of: number } {
	const gpu = system.gpus[0];
	if (gpu && system.accel_budget_mb > 0) return { mb: system.accel_budget_mb, where: 'GPU', of: gpu.memory_total_mb ?? system.ram_total_mb };
	return { mb: system.cpu_budget_mb, where: 'CPU', of: system.ram_total_mb };
}

/** 11.42 -> "11.4 s", 134 -> "2 min 14 s". */
export function seconds(value: number): string {
	if (value < 60) return `${value.toFixed(1)} s`;
	return `${Math.floor(value / 60)} min ${Math.round(value % 60)} s`;
}

/** Elapsed time as a running clock: 7 -> "0:07", 134 -> "2:14". */
export function elapsed(value: number): string {
	const total = Math.max(0, Math.floor(value));
	return `${Math.floor(total / 60)}:${String(total % 60).padStart(2, '0')}`;
}

const tiers: Record<string, { label: string; line: string }> = {
	cpu8: { label: 'Light rig', line: 'Small chat builds and voice fit here, on the processor.' },
	igpu16: { label: 'Everyday rig', line: 'Mid-size chat builds and voice fit. Image is slow.' },
	vram8: { label: 'GPU rig', line: 'Large chat builds and fast image fit on the graphics card.' },
	mac16: { label: 'Apple silicon rig', line: 'Mid-size chat builds, image and voice fit in unified memory.' },
	big: { label: 'Big rig', line: 'There is room for the largest builds in every family.' }
};

export function tierOf(system: System) {
	return tiers[system.tier] ?? { label: 'Your rig', line: '' };
}

const backends: Record<string, string> = { metal: 'Metal', cuda: 'CUDA', vulkan: 'Vulkan', cpu: 'CPU' };
export const backendName = (id: string) => backends[id] ?? id;

const osNames: Record<string, string> = { macos: 'macOS', windows: 'Windows', linux: 'Linux' };
export const osName = (id: string) => osNames[id] ?? id;

/** "m5max" -> "M5 Max", "rtx3060" -> "RTX 3060". */
function chipName(slug: string): string {
	return slug
		.replace(/^rtx(\d+)/i, 'RTX $1')
		.replace(/^m(\d)(max|pro|ultra)?$/i, (_, n, tail) => `M${n}${tail ? ' ' + tail[0].toUpperCase() + tail.slice(1) : ''}`);
}

export interface Measure {
	value: string;
	label: string;
}

/** Turns a variant's `metrics` into readable measurements. Unknown keys are shown as they are. */
export function measures(metrics: Metrics): { list: Measure[]; notes: string[] } {
	const list: Measure[] = [];
	const notes: string[] = [];
	const num = (v: unknown) => (typeof v === 'number' ? v : null);

	for (const [key, raw] of Object.entries(metrics)) {
		const n = num(raw);
		const speed = key.match(/^([a-z]+)_([a-z0-9]+)_tok_s$/);
		if (speed && n !== null) {
			list.push({ value: `${n.toFixed(1)} tokens/s`, label: `${backendName(speed[1])} on ${chipName(speed[2])}` });
		} else if (key === 'same_top_token' && n !== null) {
			list.push({ value: percent(n), label: 'same next word as the full-precision model' });
		} else if (key === 'kld_mean' && n !== null) {
			list.push({ value: n.toFixed(4), label: 'KL divergence from full precision' });
		} else if (key === 'ppl_ratio' && n !== null) {
			list.push({ value: `${((n - 1) * 100).toFixed(1)}%`, label: 'higher perplexity than full precision' });
		} else if (key === 'lpips' && n !== null) {
			list.push({ value: n.toFixed(3), label: 'LPIPS distance from full precision' });
		} else if (key === 'psnr' && n !== null) {
			list.push({ value: `${n.toFixed(1)} dB`, label: 'PSNR against full precision' });
		} else if (key === 'ssim' && n !== null) {
			list.push({ value: n.toFixed(3), label: 'SSIM against full precision' });
		} else if (key === 'roundtrip_wer' && n !== null) {
			list.push({ value: percent(n), label: 'words wrong when its speech is transcribed back' });
		} else if (key === 'rtf_1_thread' && n !== null) {
			list.push({ value: `${n.toFixed(2)}x`, label: 'real-time factor on one thread' });
		} else if (key === 'reference' && typeof raw === 'string') {
			notes.push(`Measured against ${raw}.`);
		} else if (key === 'memory_estimated') {
			if (raw === true) notes.push('Memory is an estimate, not a measured peak.');
		} else if (key === 'n') {
			// sample count; already part of the reference note
		} else if (typeof raw === 'string') {
			// Free text is a remark, not a measurement.
			notes.push(sentence(`${key.replaceAll('_', ' ')}: ${raw}`));
		} else if (raw !== null && typeof raw !== 'boolean') {
			list.push({ value: String(n !== null ? +n.toFixed(4) : raw), label: key.replaceAll('_', ' ') });
		}
	}
	return { list, notes };
}

/** First tokens/s figure in a variant's metrics, for the builds table. */
export function speedOf(metrics: Metrics): string {
	for (const [key, raw] of Object.entries(metrics)) {
		if (key.endsWith('_tok_s') && typeof raw === 'number') return `${raw.toFixed(1)} tok/s`;
	}
	return '';
}

export const sentence = (text: string) => (text ? text[0].toUpperCase() + text.slice(1) + (/[.!?]$/.test(text) ? '' : '.') : '');

/** "14:02", for transcript lines. */
export function minute(ms: number): string {
	return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', hour12: false });
}

export function clock(ms: number): string {
	return new Date(ms).toLocaleTimeString([], { hour: '2-digit', minute: '2-digit', second: '2-digit', hour12: false });
}

export function day(ms: number): string {
	return new Date(ms).toLocaleDateString([], { year: 'numeric', month: 'long', day: 'numeric' });
}

export interface Fact {
	icon: 'rig' | 'memory' | 'gpu' | 'disk' | 'system';
	label: string;
	value: string;
	detail: string;
}

/** The hardware profile as a short list of readable facts. */
export function rigFacts(system: System): Fact[] {
	const { cpu } = system;
	const cores = cpu.physical_cores === cpu.logical_cores ? `${cpu.physical_cores} cores` : `${cpu.physical_cores} cores, ${cpu.logical_cores} threads`;
	const facts: Fact[] = [
		{ icon: 'rig', label: 'Processor', value: cpu.brand, detail: [cores, ...cpu.features.map((f) => f.toUpperCase())].join(', ') },
		{ icon: 'memory', label: 'Memory', value: size(system.ram_total_mb), detail: `${size(system.ram_available_mb)} free when Rigbrew started` }
	];
	if (system.gpus.length === 0) {
		facts.push({ icon: 'gpu', label: 'Graphics', value: 'No supported GPU', detail: 'Builds run on the processor' });
	}
	for (const gpu of system.gpus) {
		const memory = gpu.memory_total_mb ? `${size(gpu.memory_total_mb)} ${gpu.kind === 'unified' ? 'shared with the system' : 'of its own'}` : '';
		facts.push({ icon: 'gpu', label: 'Graphics', value: gpu.name, detail: [backendName(gpu.backend), memory].filter(Boolean).join(', ') });
	}
	facts.push({ icon: 'disk', label: 'Storage', value: `${size(system.storage.free_mb)} free`, detail: system.storage.data_dir });
	facts.push({ icon: 'system', label: 'System', value: `${osName(system.os.family)} ${system.os.version}`, detail: system.os.arch });
	return facts;
}
