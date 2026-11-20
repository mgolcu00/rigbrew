// Plain words for what the service reports: what a model is for, which version is which, how
// fast and how big things are, said for someone who has never heard of a "model". The levels
// (experience.svelte.ts) decide which of these a screen shows; this file only knows how to say them.

import type { Flags } from './experience.svelte';
import { familyOf, installedVariant, size, variantName } from './format';
import type { Package, System, Variant } from './types';

const platform = typeof navigator === 'undefined' ? '' : navigator.platform.toLowerCase();
export const isMac = platform.includes('mac');
/** "Mac" on a Mac, "computer" elsewhere: "Uses 4 GB of your Mac's memory". */
export const machine = isMac ? 'Mac' : 'computer';
/** The modifier key as people see it on their keyboard. */
export const mod = isMac ? '⌘' : 'Ctrl+';

/** What each kind of model is for, as a word that can stand in front of its name. */
const roles: Record<string, { role: string; plain: string }> = {
	chat: { role: 'Chat', plain: 'Answers questions, writes and explains.' },
	image: { role: 'Pictures', plain: 'Makes a picture from a description.' },
	video: { role: 'Video', plain: 'Makes a short clip from a description, in a few minutes.' },
	voice: { role: 'Voice', plain: 'Reads answers aloud, in English.' },
	listen: { role: 'Listening', plain: 'Turns speech into text.' }
};

/** "Chat", "Pictures", "Video", "Voice", "Listening". */
export const roleOf = (pkg: Package) => roles[pkg.modality]?.role ?? familyOf(pkg).does;

/** One plain sentence on what the model does. */
export const plainDoes = (pkg: Package) => roles[pkg.modality]?.plain ?? `${familyOf(pkg).can}.`;

/** Function first when the level asks for plain names: "Pictures (Crema 6B)"; otherwise "Crema 6B". */
export const nameOf = (pkg: Package, flags: Pick<Flags, 'plainNames'>) => (flags.plainNames ? `${roleOf(pkg)} (${pkg.name})` : pkg.name);

/** The same, from a package id that may not be in the catalogue any more. */
export function nameById(packages: Package[], id: string | undefined | null, flags: Pick<Flags, 'plainNames'>, fallback = 'the model') {
	const pkg = packages.find((p) => p.id === id);
	return pkg ? nameOf(pkg, flags) : (id ?? fallback);
}

// --- versions ----------------------------------------------------------------------------------

const qualityKnown = (v: Variant) => v.metrics.quality_measured !== false;

/**
 * Friendly names for a package's versions, from what was measured: the smallest one, the one
 * with the best quality, the "balanced" one (the smallest that keeps 95% of the best quality),
 * and "Small" / "Better" for the rest by where they fall between those. Without quality
 * measurements, memory decides.
 */
export function versionNames(pkg: Package): Map<string, string> {
	const out = new Map<string, string>();
	const list = [...pkg.variants].sort((a, b) => a.accel_mem_mb - b.accel_mem_mb);
	if (list.length === 0) return out;
	if (list.length === 1) {
		out.set(list[0].id, 'Standard');
		return out;
	}
	const measured = list.every(qualityKnown);
	const quality = (v: Variant) => (measured ? v.quality : v.accel_mem_mb);
	const best = list.reduce((a, b) => (quality(b) > quality(a) ? b : a));
	const smallest = list[0];
	if (list.length === 2) {
		out.set(smallest.id, 'Smaller');
		out.set(best === smallest ? list[1].id : best.id, 'Better quality');
		return out;
	}
	const balanced = list.find((v) => v !== smallest && v !== best && quality(v) >= quality(best) * 0.95) ?? list[Math.floor(list.length / 2)];
	const at = (v: Variant) => list.indexOf(v);
	for (const v of list) {
		if (v === smallest) out.set(v.id, 'Smallest');
		else if (v === best) out.set(v.id, 'Best quality');
		else if (v === balanced) out.set(v.id, 'Balanced');
		else out.set(v.id, at(v) < at(balanced) ? 'Small' : 'Better');
	}
	return out;
}

/** "Balanced", "Best quality"… for one version. */
export const versionName = (pkg: Package, variant: string) => versionNames(pkg).get(variant) ?? variantName(variant);

/**
 * What to put after a model's name for one of its versions, per level: the raw id in Pro
 * ("q5"), the friendly name in Standard ("Balanced"), nothing in Guided.
 */
export function versionLabel(pkg: Package, variant: string | undefined, flags: Pick<Flags, 'buildIds' | 'versionNames'>) {
	if (!variant) return '';
	if (flags.buildIds) return variantName(variant);
	if (flags.versionNames) return versionName(pkg, variant);
	return '';
}

/** "Steep 4B · Balanced" (Standard), "Steep 4B q5" (Pro), "Chat (Steep 4B)" (Guided). */
export function fullName(pkg: Package, variant: string | undefined, flags: Flags) {
	const name = nameOf(pkg, flags);
	const version = versionLabel(pkg, variant ?? installedVariant(pkg)?.id, flags);
	if (!version) return name;
	return flags.buildIds ? `${name} ${version}` : `${name} · ${version}`;
}

// --- speed ---------------------------------------------------------------------------------------

/** Tokens per second as a word. Tokens are pieces of words, so the number is never called "words". */
export function speedWord(tokensPerSecond: number): string {
	if (tokensPerSecond >= 45) return 'very fast';
	if (tokensPerSecond >= 20) return 'fast';
	if (tokensPerSecond >= 8) return 'steady';
	return 'slow';
}

const squash = (text: string) => text.toLowerCase().replace(/[^a-z0-9]/g, '');

/**
 * The measured speed of a version on this very machine, when the catalogue has one: the
 * metric `metal_m5max_tok_s` counts for an "Apple M5 Max". Null when it was measured elsewhere.
 */
export function speedHere(variant: Variant, system: System | null): number | null {
	if (!system) return null;
	const chip = squash(system.gpus[0]?.name ?? system.cpu.brand);
	for (const [key, raw] of Object.entries(variant.metrics)) {
		const m = key.match(/^([a-z]+)_([a-z0-9]+)_tok_s$/);
		if (m && typeof raw === 'number' && chip.endsWith(m[2])) return raw;
	}
	return null;
}

/** Any measured speed (Pro shows it with where it was measured). */
export function speedAnywhere(variant: Variant): number | null {
	for (const [key, raw] of Object.entries(variant.metrics)) {
		if (key.endsWith('_tok_s') && typeof raw === 'number') return raw;
	}
	return null;
}

// --- memory and fit --------------------------------------------------------------------------------

/** "Uses 4.1 GB of your Mac's memory." */
export const usesMemory = (mb: number) => `Uses ${size(mb)} of your ${machine}’s memory`;

/** How full the memory models may use is: for the bay's plain line. */
export function room(usedMb: number, budgetMb: number): string {
	const ratio = budgetMb > 0 ? usedMb / budgetMb : 1;
	if (ratio < 0.5) return 'plenty of room';
	if (ratio < 0.85) return 'room to spare';
	return 'nearly full';
}

/** One plain verdict per model for this machine, with the green "fits" meaning kept. */
export function verdictOf(pkg: Package, system: System | null): { text: string; fits: boolean } {
	const rec = pkg.recommendation;
	if (rec.verdict === 'too_heavy') return { text: `Too big for this ${machine}’s memory`, fits: false };
	if (rec.verdict === 'needs_disk') return { text: `Needs ${size(rec.needed_mb - rec.free_mb)} more free disk space`, fits: false };
	const variant = pkg.variants.find((v) => v.id === rec.variant);
	const speed = variant ? speedHere(variant, system) : null;
	if (speed !== null && speed >= 20) return { text: `Fast on this ${machine}`, fits: true };
	return { text: `Works well on this ${machine}`, fits: true };
}

// --- the machine ----------------------------------------------------------------------------------

const tiers: Record<string, string> = {
	cpu8: 'Room for small chat models and voice. They run on the processor.',
	igpu16: 'Room for mid-size chat models and voice. Pictures are slow.',
	vram8: 'Room for large chat models and fast pictures on the graphics card.',
	mac16: 'Room for mid-size chat models, pictures and voice.',
	big: 'Room for every model Rigbrew offers, the largest versions included.'
};

/** One plain sentence on what this machine can run. */
export const machineLine = (system: System) => tiers[system.tier] ?? 'Rigbrew picked the versions that fit.';
