<script lang="ts">
	import { onDestroy, onMount, tick, untrack } from 'svelte';
	import { dbLabel, dropPlace, joinFromDrag, newId, regionEnd, regionSpan, trimRegionEnd, trimRegionStart, trimShot, type DocRegion, type DocTrack } from '#lib/studio-edit.ts';
	import { LOOK, STATE_COLOR, alpha, darker, fileName, firstWords, lighter } from '#lib/studio-look.ts';
	import { FILM, SHOTS, TRACK_COLORS, studio } from '#lib/studio.svelte.ts';
	import { edgeScroll, fitZoom, followScroll, rulerLabel, rulerSteps, shortTime, snapFrame, snapTime, timeToX, timecode, xToTime, zoomAround, type Zoom } from '#lib/studio-time.ts';
	import type { FilmPart } from '#lib/studio.ts';
	import StudioFader from './StudioFader.svelte';
	import StudioIcon from './StudioIcon.svelte';
	import StudioKnob from './StudioKnob.svelte';
	import StudioMeter from './StudioMeter.svelte';

	// The tracks area: a ruler, the track headers, and the lanes. The lanes and the ruler are
	// drawn on canvas, only what is in view; the playhead is one element moved over them, so
	// nothing is laid out again while the film plays.

	const RULER = 28;
	/** The upper part of the ruler, where the cycle range is set. */
	const CYCLE = 11;
	const LABEL = 14;
	const INSET = 1;

	interface Row {
		id: string;
		kind: 'film' | 'shots' | 'track';
		top: number;
		height: number;
		track?: DocTrack;
	}

	type Hit =
		| { what: 'shot'; id: string; part: FilmPart; zone: 'body' | 'left' | 'right' }
		| { what: 'join'; id: string; part: FilmPart }
		| { what: 'gain'; id: string; part: FilmPart }
		| { what: 'region'; id: string; track: string; zone: 'body' | 'left' | 'right' | 'fadein' | 'fadeout' }
		| { what: 'lane'; row: Row | null };

	let root = $state<HTMLDivElement | null>(null);
	let lanes = $state<HTMLDivElement | null>(null);
	let canvas = $state<HTMLCanvasElement | null>(null);
	let ruler = $state<HTMLCanvasElement | null>(null);
	let headsInner = $state<HTMLDivElement | null>(null);
	let playhead = $state<HTMLDivElement | null>(null);
	let playcap = $state<HTMLDivElement | null>(null);
	let width = $state(0);
	let height = $state(0);
	let scrollX = 0;
	let scrollY = 0;
	let fonts = { text: 'sans-serif', data: 'monospace' };

	const vz = $derived(studio.layout.vzoom);
	const rows = $derived.by(() => {
		const out: Row[] = [];
		let top = 0;
		const add = (id: string, kind: Row['kind'], h: number, track?: DocTrack) => {
			out.push({ id, kind, top, height: Math.round(h * vz), track });
			top += Math.round(h * vz);
		};
		add(FILM, 'film', 78);
		add(SHOTS, 'shots', 44);
		for (const t of studio.doc?.tracks ?? []) add(t.id, 'track', 58, t);
		return out;
	});
	const rowsEnd = $derived(rows.length ? rows[rows.length - 1].top + rows[rows.length - 1].height : 0);
	const contentWidth = $derived(Math.max(width, Math.ceil(studio.length * studio.pps + Math.max(240, width * 0.6))));
	const contentHeight = $derived(Math.max(height, rowsEnd + 60));
	const zoom = (): Zoom => ({ pps: studio.pps, scroll: scrollX });

	// --- what the pointer does ---------------------------------------------------------------------

	interface Drag {
		kind: 'shots' | 'trim' | 'join' | 'gain' | 'regions' | 'rtrim' | 'fade' | 'band' | 'scrub' | 'cycle';
		/** Where it started, in pixels of the lanes' window, and in seconds and pixels of the content. */
		x0: number;
		y0: number;
		t0: number;
		cy0: number;
		x: number;
		y: number;
		moved: boolean;
		copy: boolean;
		free: boolean;
		id: string;
		edge: 'left' | 'right';
		/** What the dragged things were when the drag started. */
		shot?: { trim_start: number; trim_end: number; volume: number; clip: number };
		part?: FilmPart;
		regions?: Map<string, { row: number; region: DocRegion }>;
		ids?: string[];
		place?: number;
		pointer: number;
		from: 'lanes' | 'ruler';
	}
	let drag: Drag | null = null;
	let dragging = $state('');
	let tip = $state<{ x: number; y: number; text: string } | null>(null);
	let band = $state<{ x: number; y: number; w: number; h: number } | null>(null);
	/** A line where something snapped, and where a dragged shot or a dropped file would land. */
	let snapAt: number | null = null;
	let dropAt: { row: string; time: number } | null = null;
	let hoverJoin = '';
	let hoverRegion = '';
	let auto = 0;
	let autoFrame = 0;

	const rowAt = (cy: number) => rows.find((r) => cy >= r.top && cy < r.top + r.height) ?? null;
	const rowOf = (id: string) => rows.find((r) => r.id === id);

	/** What is under a point of the lanes' window. */
	function hitTest(x: number, y: number): Hit {
		const z = zoom();
		const cy = y + scrollY;
		const row = rowAt(cy);
		if (!row) return { what: 'lane', row: null };
		const t = xToTime(x, z);
		const parts = studio.plan.parts;
		if (row.kind === 'film') {
			const inLabel = cy - row.top < LABEL + 4;
			// The join handle sits on the cut, in the label bar of the two shots.
			if (inLabel) {
				for (let i = 1; i < parts.length; i++) {
					const p = parts[i];
					if (studio.docShot(p.shot)?.continues) continue;
					const jx = timeToX(p.start + p.overlap / 2, z);
					if (Math.abs(x - jx) <= 7) return { what: 'join', id: p.shot, part: p };
				}
			}
			for (let i = parts.length - 1; i >= 0; i--) {
				const p = parts[i];
				const x0 = timeToX(p.start, z);
				const x1 = timeToX(p.start + p.length, z);
				if (x < x0 || x > x1) continue;
				const grip = Math.min(6, (x1 - x0) / 3);
				if (p.made && x - x0 <= grip) return { what: 'shot', id: p.shot, part: p, zone: 'left' };
				if (p.made && x1 - x <= grip) return { what: 'shot', id: p.shot, part: p, zone: 'right' };
				return { what: 'shot', id: p.shot, part: p, zone: 'body' };
			}
			return { what: 'lane', row };
		}
		if (row.kind === 'shots') {
			const p = parts.find((q) => q.made && t >= q.start && t < q.start + q.length);
			if (!p) return { what: 'lane', row };
			const shot = studio.docShot(p.shot);
			const gy = row.top + gainY(row, shot?.volume ?? 1);
			if (Math.abs(cy - gy) <= 4) return { what: 'gain', id: p.shot, part: p };
			return { what: 'shot', id: p.shot, part: p, zone: 'body' };
		}
		const regions = row.track?.regions ?? [];
		for (let i = regions.length - 1; i >= 0; i--) {
			const r = regions[i];
			const x0 = timeToX(r.start, z);
			const x1 = timeToX(regionEnd(r), z);
			if (x < x0 - 2 || x > x1 + 2) continue;
			const wide = x1 - x0;
			// Fade handles: at the top corners, where each fade ends.
			if (cy - row.top < LABEL + 6 && wide > 30) {
				const fi = timeToX(r.start + r.fade_in, z);
				const fo = timeToX(regionEnd(r) - r.fade_out, z);
				if (Math.abs(x - fi) <= 6 && (r.fade_in > 0 || x - x0 <= 12)) return { what: 'region', id: r.id, track: row.id, zone: 'fadein' };
				if (Math.abs(x - fo) <= 6 && (r.fade_out > 0 || x1 - x <= 12)) return { what: 'region', id: r.id, track: row.id, zone: 'fadeout' };
			}
			const grip = Math.min(6, wide / 3);
			if (x - x0 <= grip && r.loops <= 1) return { what: 'region', id: r.id, track: row.id, zone: 'left' };
			if (x1 - x <= grip) return { what: 'region', id: r.id, track: row.id, zone: 'right' };
			return { what: 'region', id: r.id, track: row.id, zone: 'body' };
		}
		return { what: 'lane', row };
	}

	/** Where the gain line of the shots' sound stands in its row: 0 at the bottom, 150% at the top. */
	const gainY = (row: Row, volume: number) => row.height - 3 - (Math.min(1.5, Math.max(0, volume)) / 1.5) * (row.height - 8);

	/** The moments an edge may snap to: the edges of everything that is not being dragged, and the playhead. */
	function marks(skip: string[] = []) {
		const out = [0, studio.time];
		for (const p of studio.plan.parts) out.push(p.start, p.start + p.length);
		for (const t of studio.doc?.tracks ?? []) for (const r of t.regions) if (!skip.includes(r.id)) out.push(r.start, regionEnd(r));
		if (studio.cycle.to > studio.cycle.from) out.push(studio.cycle.from, studio.cycle.to);
		return out;
	}

	function cursorOf(hit: Hit) {
		if (hit.what === 'join') return 'col-resize';
		if (hit.what === 'gain') return 'ns-resize';
		if (hit.what === 'shot') return hit.zone === 'left' ? 'w-resize' : hit.zone === 'right' ? 'e-resize' : 'default';
		if (hit.what === 'region') return hit.zone === 'left' ? 'w-resize' : hit.zone === 'right' ? 'e-resize' : hit.zone === 'fadein' ? 'nesw-resize' : hit.zone === 'fadeout' ? 'nwse-resize' : 'default';
		return 'default';
	}

	const local = (e: { clientX: number; clientY: number }, el: HTMLElement) => {
		const box = el.getBoundingClientRect();
		return { x: e.clientX - box.left, y: e.clientY - box.top };
	};

	function down(e: PointerEvent) {
		if (e.button !== 0 || !canvas || !studio.doc) return;
		const { x, y } = local(e, canvas);
		const hit = hitTest(x, y);
		const add = e.shiftKey || e.metaKey || e.ctrlKey;
		const base = { x0: x, y0: y, t0: xToTime(x, zoom()), cy0: y + scrollY, x, y, moved: false, copy: false, free: false, id: '', edge: 'left' as const, pointer: e.pointerId, from: 'lanes' as const };
		canvas.setPointerCapture(e.pointerId);
		(document.activeElement as HTMLElement | null)?.blur?.();
		if (hit.what === 'shot') {
			const shot = studio.docShot(hit.id);
			if (hit.zone === 'body' || !shot) {
				const was = studio.shotsSel.includes(hit.id);
				if (add) studio.selectShot(hit.id, true);
				else if (!was) studio.selectShot(hit.id);
				else {
					// A press on one of several selected shots keeps them all, to move them together.
					studio.regionsSel = [];
					studio.trackSel = false;
					studio.track = FILM;
				}
				drag = { ...base, kind: 'shots', id: hit.id, ids: was && !add ? [...studio.shotsSel] : [hit.id] };
			} else {
				studio.selectShot(hit.id);
				studio.begin();
				drag = { ...base, kind: 'trim', id: hit.id, edge: hit.zone, part: hit.part, shot: { trim_start: shot.trim_start, trim_end: shot.trim_end, volume: shot.volume, clip: studio.clipSeconds(hit.id) } };
			}
		} else if (hit.what === 'join') {
			studio.selectShot(hit.id);
			studio.begin();
			drag = { ...base, kind: 'join', id: hit.id, part: hit.part };
		} else if (hit.what === 'gain') {
			const shot = studio.docShot(hit.id)!;
			studio.selectShot(hit.id);
			studio.track = SHOTS;
			studio.begin();
			drag = { ...base, kind: 'gain', id: hit.id, part: hit.part, shot: { trim_start: 0, trim_end: 0, volume: shot.volume, clip: 0 } };
		} else if (hit.what === 'region') {
			const was = studio.regionsSel.includes(hit.id);
			if (hit.zone === 'body') {
				if (add) studio.selectRegion(hit.id, true);
				else if (!was) studio.selectRegion(hit.id);
				else studio.track = hit.track;
				drag = { ...base, kind: 'regions', id: hit.id, copy: e.altKey };
			} else {
				studio.selectRegion(hit.id);
				studio.begin();
				const found = studio.region(hit.id)!;
				const regions = new Map([[hit.id, { row: 0, region: { ...found.region } }]]);
				drag = hit.zone === 'left' || hit.zone === 'right' ? { ...base, kind: 'rtrim', id: hit.id, edge: hit.zone, regions } : { ...base, kind: 'fade', id: hit.id, edge: hit.zone === 'fadein' ? 'left' : 'right', regions };
			}
		} else {
			if (!add) studio.selectNothing();
			if (hit.row) studio.track = hit.row.id;
			drag = { ...base, kind: 'band' };
		}
		dragging = drag?.kind ?? '';
	}

	function move(e: PointerEvent) {
		if (!canvas) return;
		const { x, y } = local(e, canvas);
		if (!drag || drag.from !== 'lanes') {
			if (e.buttons === 0) {
				const hit = hitTest(x, y);
				canvas.style.cursor = cursorOf(hit);
				const join = hit.what === 'join' ? hit.id : '';
				const region = hit.what === 'region' ? hit.id : '';
				if (join !== hoverJoin || region !== hoverRegion) {
					hoverJoin = join;
					hoverRegion = region;
					draw();
				}
			}
			return;
		}
		drag.x = x;
		drag.y = y;
		drag.free = e.ctrlKey || e.metaKey || (e.altKey && !drag.copy);
		if (!drag.moved && Math.hypot(x - drag.x0, y - drag.y0) < 4) return;
		const first = !drag.moved;
		drag.moved = true;
		follow(e, first);
		auto = edgeScroll(x, width);
		if (auto && !autoFrame) autoFrame = requestAnimationFrame(autoscroll);
	}

	/** While something is dragged at the edge of the window, the window scrolls and the drag goes on. */
	function autoscroll() {
		autoFrame = 0;
		if (!drag || !auto || !lanes) return;
		const before = lanes.scrollLeft;
		lanes.scrollLeft = before + auto;
		if (lanes.scrollLeft !== before) follow(null, false);
		autoFrame = requestAnimationFrame(autoscroll);
	}

	/** One step of the drag in hand. */
	function follow(e: PointerEvent | null, first: boolean) {
		const d = drag;
		if (!d || !studio.doc) return;
		const z = zoom();
		const t = xToTime(d.x, z);
		const snapOn = studio.layout.snap && !d.free;
		const client = e ? { x: e.clientX, y: e.clientY } : tip ? { x: tip.x - 14, y: tip.y - 18 } : { x: 0, y: 0 };
		const say = (text: string) => (tip = { x: client.x + 14, y: client.y + 18, text });
		snapAt = null;
		if (d.kind === 'shots') {
			d.place = dropPlace(studio.plan.parts, t);
			const n = d.ids?.length ?? 1;
			say(n === 1 ? `Move shot to place ${Math.min(studio.plan.parts.length, d.place + 1)}` : `Move ${n} shots`);
		} else if (d.kind === 'trim' && d.shot) {
			const delta = t - d.t0;
			const next = trimShot(d.shot, d.shot.clip, d.edge === 'left' ? 'start' : 'end', delta);
			studio.live(() => Object.assign(studio.docShot(d.id) ?? {}, next));
			const kept = d.shot.clip - next.trim_start - next.trim_end;
			say(`${d.edge === 'left' ? `Start cut ${shortTime(next.trim_start)}` : `End cut ${shortTime(next.trim_end)}`}  ·  Length ${shortTime(kept)}`);
		} else if (d.kind === 'join' && d.part) {
			const row = rowOf(FILM)!;
			const next = joinFromDrag(t - d.t0, d.y + scrollY < row.top + row.height / 2);
			studio.live(() => Object.assign(studio.docShot(d.id) ?? {}, next));
			say(next.join === 'cut' ? 'Cut' : `${next.join === 'dissolve' ? 'Dissolve' : 'Fade through black'} ${shortTime(next.join_seconds)}`);
		} else if (d.kind === 'gain' && d.shot) {
			const row = rowOf(SHOTS)!;
			const by = ((d.y0 - d.y) / (row.height - 8)) * 1.5 * (e?.shiftKey ? 0.25 : 1);
			const volume = Math.round(Math.min(1.5, Math.max(0, d.shot.volume + by)) * 100) / 100;
			studio.live(() => {
				const s = studio.docShot(d.id);
				if (s) s.volume = volume;
			});
			say(`${dbLabel(volume)} dB`);
		} else if (d.kind === 'regions') {
			if (first) startRegions(d);
			moveRegions(d, t, snapOn, say);
		} else if (d.kind === 'rtrim' && d.regions) {
			const orig = d.regions.get(d.id)!.region;
			const snapped = snapTime(t, marks([d.id]), z.pps, snapOn);
			snapAt = snapped.mark;
			const next = d.edge === 'left' ? trimRegionStart(orig, snapped.time) : trimRegionEnd(orig, snapped.time, studio.fileSeconds(orig.file));
			replaceRegion(next);
			say(orig.loops > 1 ? `${next.loops} times  ·  Length ${shortTime(regionSpan(next))}` : `${d.edge === 'left' ? `Start ${shortTime(next.start)}` : `End ${shortTime(regionEnd(next))}`}  ·  Length ${shortTime(next.length)}`);
		} else if (d.kind === 'fade' && d.regions) {
			const orig = d.regions.get(d.id)!.region;
			const span = regionSpan(orig);
			const at = snapOn ? snapFrame(t) : t;
			const next = d.edge === 'left' ? { ...orig, fade_in: Math.min(Math.max(0, at - orig.start), span - orig.fade_out) } : { ...orig, fade_out: Math.min(Math.max(0, regionEnd(orig) - at), span - orig.fade_in) };
			replaceRegion(next);
			say(d.edge === 'left' ? `Fade in ${shortTime(next.fade_in)}` : `Fade out ${shortTime(next.fade_out)}`);
		} else if (d.kind === 'band') {
			const x0 = Math.min(d.t0 * z.pps, d.x + scrollX);
			const x1 = Math.max(d.t0 * z.pps, d.x + scrollX);
			const y0 = Math.min(d.cy0, d.y + scrollY);
			const y1 = Math.max(d.cy0, d.y + scrollY);
			band = { x: x0 - scrollX, y: y0 - scrollY, w: x1 - x0, h: y1 - y0 };
			const [from, to] = [x0 / z.pps, x1 / z.pps];
			const started = rowAt(d.cy0);
			// A band that starts on the film selects shots; one that starts on a sound track, regions.
			if (!started || started.kind !== 'track') {
				const picked = rows.some((r) => r.kind !== 'track' && r.top < y1 && r.top + r.height > y0) ? studio.plan.parts.filter((p) => p.start < to && p.start + p.length > from).map((p) => p.shot) : [];
				studio.regionsSel = [];
				studio.shotsSel = picked;
				if (started === null && picked.length === 0) pickRegions(from, to, y0, y1);
			} else pickRegions(from, to, y0, y1);
		}
		draw();
	}

	function pickRegions(from: number, to: number, y0: number, y1: number) {
		const picked: string[] = [];
		for (const row of rows) {
			if (row.kind !== 'track' || row.top >= y1 || row.top + row.height <= y0) continue;
			for (const r of row.track!.regions) if (r.start < to && regionEnd(r) > from) picked.push(r.id);
		}
		studio.shotsSel = [];
		studio.regionsSel = picked;
	}

	function replaceRegion(next: DocRegion) {
		studio.live((doc) => {
			for (const t of doc.tracks) {
				const at = t.regions.findIndex((r) => r.id === next.id);
				if (at >= 0) t.regions[at] = next;
			}
		});
	}

	/** A drag of regions starts: what they were is kept, and with Alt held, copies are what moves. */
	function startRegions(d: Drag) {
		studio.begin();
		const tracks = studio.doc!.tracks;
		if (d.copy) {
			const fresh: string[] = [];
			let grabbed = d.id;
			studio.live((doc) => {
				for (const t of doc.tracks) {
					for (const r of [...t.regions]) {
						if (!studio.regionsSel.includes(r.id)) continue;
						const copy = { ...r, id: newId() };
						if (r.id === d.id) grabbed = copy.id;
						fresh.push(copy.id);
						t.regions.push(copy);
					}
				}
			});
			studio.regionsSel = fresh;
			d.id = grabbed;
		}
		d.regions = new Map();
		tracks.forEach((t, row) => {
			for (const r of t.regions) if (studio.regionsSel.includes(r.id)) d.regions!.set(r.id, { row, region: { ...r } });
		});
	}

	function moveRegions(d: Drag, t: number, snapOn: boolean, say: (text: string) => void) {
		const origs = d.regions!;
		const grabbed = origs.get(d.id)?.region;
		if (!grabbed) return;
		const tracks = studio.doc!.tracks;
		const z = zoom();
		// The grabbed region's start or end snaps, whichever is nearer a mark; the others keep their distance.
		const wanted = grabbed.start + (t - d.t0);
		const skip = [...origs.keys()];
		const atStart = snapTime(wanted, marks(skip), z.pps, snapOn);
		const atEnd = snapTime(wanted + regionSpan(grabbed), marks(skip), z.pps, snapOn);
		let start = atStart.time;
		snapAt = atStart.mark;
		if (atStart.mark === null && atEnd.mark !== null) {
			start = atEnd.time - regionSpan(grabbed);
			snapAt = atEnd.mark;
		}
		const earliest = Math.min(...[...origs.values()].map((o) => o.region.start));
		const by = Math.max(-earliest, start - grabbed.start);
		// Up or down by whole tracks, as far as every dragged region still has a track.
		const firstTrack = rows.findIndex((r) => r.kind === 'track');
		const startRow = rows.findIndex((r) => r.id === rowAt(d.cy0)?.id) - firstTrack;
		const under = rowAt(d.y + scrollY);
		const nowRow = under ? rows.indexOf(under) - firstTrack : d.y + scrollY < (rows[firstTrack]?.top ?? 0) ? 0 : tracks.length - 1;
		const rowsOf = [...origs.values()].map((o) => o.row);
		const shift = Math.min(tracks.length - 1 - Math.max(...rowsOf), Math.max(-Math.min(...rowsOf), nowRow - startRow));
		studio.live((doc) => {
			for (const tr of doc.tracks) tr.regions = tr.regions.filter((r) => !origs.has(r.id));
			for (const [, o] of origs) doc.tracks[o.row + shift]?.regions.push({ ...o.region, start: Math.max(0, o.region.start + by) });
		});
		if (shift !== 0) studio.track = tracks[origs.get(d.id)!.row + shift]?.id ?? studio.track;
		say(`${shortTime(grabbed.start + by)}  ·  ${by >= 0 ? '+' : ''}${shortTime(by)}`);
	}

	function up(e: PointerEvent) {
		const d = drag;
		if (!d || d.from !== 'lanes') return;
		drag = null;
		dragging = '';
		tip = null;
		band = null;
		snapAt = null;
		auto = 0;
		canvas?.releasePointerCapture(e.pointerId);
		if (d.kind === 'shots') {
			if (d.moved && d.place !== undefined) studio.moveShotsTo(d.ids ?? [d.id], d.place);
			else if (!(e.shiftKey || e.metaKey || e.ctrlKey)) studio.shotsSel = [d.id];
		} else if (d.kind === 'trim') studio.commit('Trim shot');
		else if (d.kind === 'join') {
			if (d.moved) studio.commit('Join');
			else {
				// A click on the handle steps through the three joins.
				studio.cancel();
				studio.edit('Join', () => {
					const s = studio.docShot(d.id);
					if (s) s.join = s.join === 'cut' ? 'dissolve' : s.join === 'dissolve' ? 'black' : 'cut';
				});
			}
		} else if (d.kind === 'gain') studio.commit('Shot volume');
		else if (d.kind === 'regions') {
			if (d.moved) studio.commit(d.copy ? 'Copy region' : 'Move region');
			else if (!(e.shiftKey || e.metaKey || e.ctrlKey)) studio.selectRegion(d.id);
		} else if (d.kind === 'rtrim') studio.commit('Trim region');
		else if (d.kind === 'fade') studio.commit('Fade');
		draw();
	}

	/** Escape gives up a drag: everything is as it was. */
	function escape(e: KeyboardEvent) {
		if (e.key !== 'Escape' || !drag) return;
		e.stopPropagation();
		const d = drag;
		drag = null;
		dragging = '';
		tip = null;
		band = null;
		snapAt = null;
		auto = 0;
		if (d.kind !== 'shots' && d.kind !== 'band' && d.kind !== 'scrub' && d.kind !== 'cycle') studio.cancel();
		draw();
	}

	function dblclick(e: MouseEvent) {
		if (!canvas || !studio.doc) return;
		const { x, y } = local(e, canvas);
		const hit = hitTest(x, y);
		if (hit.what === 'lane' && hit.row?.kind === 'film' && xToTime(x, zoom()) >= studio.plan.total) studio.addShot();
		else if (hit.what === 'gain')
			studio.edit('Shot volume', () => {
				const s = studio.docShot(hit.id);
				if (s) s.volume = 1;
			});
		else if (hit.what === 'shot' || hit.what === 'region') {
			studio.set('tab', 'editor');
			studio.set('bottom', true);
		}
	}

	// --- the ruler: the playhead and the cycle range ---------------------------------------------

	function rulerDown(e: PointerEvent) {
		if (e.button !== 0 || !ruler) return;
		const { x, y } = local(e, ruler);
		ruler.setPointerCapture(e.pointerId);
		const t = xToTime(x, zoom());
		drag = { kind: y < CYCLE ? 'cycle' : 'scrub', x0: x, y0: y, t0: Math.max(0, t), cy0: 0, x, y, moved: false, copy: false, free: e.altKey, id: '', edge: 'left', pointer: e.pointerId, from: 'ruler' };
		dragging = drag.kind;
		if (drag.kind === 'scrub') scrub(x, e.altKey);
	}

	function scrub(x: number, free: boolean) {
		const z = zoom();
		const marksNow = marks().filter((m) => m !== studio.time);
		studio.seek(snapTime(xToTime(x, z), marksNow, z.pps, studio.layout.snap && !free).time);
	}

	function rulerMove(e: PointerEvent) {
		if (!ruler) return;
		const { x, y } = local(e, ruler);
		if (!drag || drag.from !== 'ruler') {
			ruler.style.cursor = y < CYCLE ? 'ew-resize' : 'default';
			return;
		}
		drag.x = x;
		if (Math.abs(x - drag.x0) >= 3) drag.moved = true;
		auto = edgeScroll(x, width);
		if (auto && !autoFrame) autoFrame = requestAnimationFrame(autoRuler);
		rulerFollow(e.altKey);
	}

	function rulerFollow(free: boolean) {
		if (!drag) return;
		const z = zoom();
		if (drag.kind === 'scrub') scrub(drag.x, free);
		else if (drag.moved) {
			const t = snapTime(xToTime(drag.x, z), marks(), z.pps, studio.layout.snap && !free).time;
			const from = Math.min(drag.t0, t);
			studio.cycle = { on: true, from: snapFrame(from), to: Math.max(snapFrame(from) + 1 / 24, Math.max(drag.t0, t)) };
		}
	}

	function autoRuler() {
		autoFrame = 0;
		if (!drag || drag.from !== 'ruler' || !auto || !lanes) return;
		lanes.scrollLeft += auto;
		rulerFollow(false);
		autoFrame = requestAnimationFrame(autoRuler);
	}

	function rulerUp(e: PointerEvent) {
		const d = drag;
		if (!d || d.from !== 'ruler') return;
		drag = null;
		dragging = '';
		auto = 0;
		ruler?.releasePointerCapture(e.pointerId);
		// A click on the cycle strip switches the cycle on and off.
		if (d.kind === 'cycle' && !d.moved) studio.toggleCycle();
	}

	// --- zoom and scroll ----------------------------------------------------------------------------

	let anchor: { x: number; time: number } | null = null;
	let lastPps = untrack(() => studio.pps);

	function wheel(e: WheelEvent) {
		if (!lanes || !canvas) return;
		if (e.ctrlKey || e.metaKey) {
			// A pinch, or the wheel with Ctrl: zoom around the pointer.
			e.preventDefault();
			const { x } = local(e, canvas);
			const next = zoomAround(zoom(), x, Math.exp(-e.deltaY * 0.012));
			anchor = { x, time: xToTime(x, zoom()) };
			studio.pps = next.pps;
		} else if (e.shiftKey && e.deltaX === 0) {
			e.preventDefault();
			lanes.scrollLeft += e.deltaY;
		}
	}

	// A new zoom keeps a moment where it is: the one under the pointer, else the playhead when it
	// is in view, else the middle of the window.
	$effect(() => {
		const pps = studio.pps;
		void contentWidth;
		if (!lanes || pps === lastPps) return;
		const px = studio.time * lastPps - scrollX;
		const at = anchor ?? (px >= 0 && px <= width ? { x: px, time: studio.time } : { x: width / 2, time: (scrollX + width / 2) / lastPps });
		anchor = null;
		lastPps = pps;
		lanes.scrollLeft = Math.max(0, at.time * pps - at.x);
		scrollX = lanes.scrollLeft;
		place();
		draw();
		drawRuler();
	});

	// Z, and a film that was just opened: the whole film in the window.
	let fitted = -1;
	$effect(() => {
		const asked = studio.fits;
		if (width <= 0 || fitted === asked) return;
		fitted = asked;
		untrack(() => {
			studio.pps = fitZoom(Math.max(studio.length, 4), width).pps;
			anchor = { x: 0, time: 0 };
			if (lanes) lanes.scrollLeft = 0;
		});
	});

	function scrolled() {
		if (!lanes) return;
		scrollX = lanes.scrollLeft;
		scrollY = lanes.scrollTop;
		if (headsInner) headsInner.style.transform = `translateY(${-scrollY}px)`;
		place();
		draw();
		drawRuler();
	}

	/** Puts the playhead where the film is. Only a transform: nothing is laid out. */
	function place() {
		const x = timeToX(studio.time, zoom());
		const shown = x >= -1 && x <= width + 1;
		if (playhead) {
			playhead.style.transform = `translateX(${Math.round(x)}px)`;
			playhead.style.visibility = shown ? 'visible' : 'hidden';
		}
		if (playcap) {
			playcap.style.transform = `translateX(${Math.round(x)}px)`;
			playcap.style.visibility = shown ? 'visible' : 'hidden';
		}
	}

	const stopFrames = studio.onFrame((time) => {
		// While the film plays, the window follows the playhead.
		if (studio.playing && lanes && !drag) {
			const next = followScroll(time, zoom(), width);
			if (next !== scrollX) lanes.scrollLeft = next;
		}
		place();
	});

	// --- drawing ------------------------------------------------------------------------------------

	const tiles = new Map<string, HTMLCanvasElement>();
	/** A poster scaled once to the height of the film track. */
	function tile(key: string, image: HTMLImageElement, h: number, dpr: number) {
		const id = `${key}@${h}@${dpr}`;
		let made = tiles.get(id);
		if (!made) {
			made = document.createElement('canvas');
			made.height = Math.max(1, Math.round(h * dpr));
			made.width = Math.max(1, Math.round(((h * image.naturalWidth) / Math.max(1, image.naturalHeight)) * dpr));
			made.getContext('2d')!.drawImage(image, 0, 0, made.width, made.height);
			if (tiles.size > 400) tiles.clear();
			tiles.set(id, made);
		}
		return made;
	}

	function fit(g: CanvasRenderingContext2D, w: number, h: number, el: HTMLCanvasElement) {
		const dpr = window.devicePixelRatio || 1;
		if (el.width !== Math.round(w * dpr) || el.height !== Math.round(h * dpr)) {
			el.width = Math.round(w * dpr);
			el.height = Math.round(h * dpr);
		}
		g.setTransform(dpr, 0, 0, dpr, 0, 0);
		g.clearRect(0, 0, w, h);
		return dpr;
	}

	function box(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, r = 3) {
		g.beginPath();
		g.roundRect(x, y, Math.max(0, w), Math.max(0, h), Math.min(r, Math.max(0, w) / 2));
	}

	/** The body of a region: a soft gradient of its colour with a light line along its top. */
	function body(g: CanvasRenderingContext2D, x: number, y: number, w: number, h: number, color: string, on: boolean) {
		const fill = g.createLinearGradient(0, y, 0, y + h);
		fill.addColorStop(0, lighter(color, on ? 0.3 : 0.12));
		fill.addColorStop(1, darker(color, on ? 0.02 : 0.16));
		box(g, x, y, w, h);
		g.fillStyle = fill;
		g.fill();
		g.save();
		g.clip();
		g.fillStyle = 'rgba(255,255,255,0.16)';
		g.fillRect(x, y, w, 1);
		g.restore();
	}

	/** A waveform between two moments of a sound, as one filled shape. */
	function wave(g: CanvasRenderingContext2D, pairs: Int8Array, rate: number, from: number, x0: number, x1: number, pps: number, mid: number, half: number, gain = 1) {
		const a = Math.max(0, Math.floor(x0));
		const b = Math.min(width, Math.ceil(x1));
		if (b <= a || pairs.length === 0) return;
		const tops: number[] = [];
		const bottoms: number[] = [];
		const count = pairs.length / 2;
		for (let px = a; px < b; px++) {
			const i0 = Math.floor((from + (px - x0) / pps) * rate);
			const i1 = Math.max(i0 + 1, Math.floor((from + (px + 1 - x0) / pps) * rate));
			let lo = 0;
			let hi = 0;
			for (let i = Math.max(0, i0); i < Math.min(count, i1); i++) {
				if (pairs[i * 2] < lo) lo = pairs[i * 2];
				if (pairs[i * 2 + 1] > hi) hi = pairs[i * 2 + 1];
			}
			tops.push(mid - Math.min(1, (hi / 127) * gain) * half - 0.5);
			bottoms.push(mid - Math.max(-1, (lo / 127) * gain) * half + 0.5);
		}
		g.beginPath();
		g.moveTo(a, tops[0]);
		for (let i = 1; i < tops.length; i++) g.lineTo(a + i, tops[i]);
		for (let i = bottoms.length - 1; i >= 0; i--) g.lineTo(a + i, bottoms[i]);
		g.closePath();
		g.fill();
	}

	function label(g: CanvasRenderingContext2D, text: string, x: number, y: number, w: number, color: string, weight = 500) {
		if (w < 14) return;
		g.save();
		g.beginPath();
		g.rect(x, y - 11, w, 15);
		g.clip();
		g.font = `${weight} 10.5px ${fonts.text}`;
		g.fillStyle = color;
		g.textBaseline = 'alphabetic';
		g.fillText(text, x, y);
		g.restore();
	}

	function draw() {
		const el = canvas;
		const doc = studio.doc;
		if (!el || !doc || width <= 0 || height <= 0) return;
		const g = el.getContext('2d')!;
		const dpr = fit(g, width, height, el);
		const z = zoom();
		const parts = studio.plan.parts;
		const sel = studio.shotsSel;

		// Lanes.
		g.fillStyle = LOOK.bg;
		g.fillRect(0, 0, width, height);
		for (const row of rows) {
			const y = row.top - scrollY;
			if (y > height || y + row.height < 0) continue;
			g.fillStyle = row.id === studio.track ? LOOK.laneOn : LOOK.lane;
			g.fillRect(0, y, width, row.height - 1);
		}
		// A faint line down the lanes at every labelled mark of the ruler.
		const steps = rulerSteps(z.pps);
		g.fillStyle = 'rgba(255,255,255,0.035)';
		for (let t = Math.floor(xToTime(0, z) / steps.major) * steps.major; t <= xToTime(width, z); t += steps.major) {
			const x = Math.round(timeToX(t, z));
			if (x >= 0) g.fillRect(x, 0, 1, Math.min(height, rowsEnd - scrollY));
		}
		// The end of the film: what lies after it is not in the exported file.
		const endX = timeToX(studio.plan.total, z);
		if (studio.plan.total > 0 && endX < width) {
			g.fillStyle = 'rgba(0,0,0,0.28)';
			g.fillRect(Math.max(0, endX), 0, width - Math.max(0, endX), Math.min(height, rowsEnd - scrollY));
			g.fillStyle = LOOK.line;
			g.fillRect(Math.round(endX), 0, 1, Math.min(height, rowsEnd - scrollY));
		}

		const film = rowOf(FILM);
		const sound = rowOf(SHOTS);
		const progress = studio.view?.progress;
		for (let i = 0; i < parts.length; i++) {
			const p = parts[i];
			const x0 = timeToX(p.start, z);
			const x1 = timeToX(p.start + p.length, z);
			if (x1 < -2 || x0 > width + 2) continue;
			const shot = studio.shots[p.index];
			if (!shot) continue;
			const on = sel.includes(p.shot);
			const w = x1 - x0 - INSET;
			if (film) {
				const y = film.top - scrollY + 2;
				const h = film.height - 5;
				const color = STATE_COLOR[shot.state] ?? STATE_COLOR.empty;
				body(g, x0, y, w, h, color, on);
				g.save();
				box(g, x0, y, w, h);
				g.clip();
				// The filmstrip: the shot's poster, again and again across its width.
				const poster = studio.posters.get(studio.posterKey(shot));
				const stripY = y + LABEL;
				const stripH = h - LABEL;
				if (poster && stripH > 6) {
					const one = tile(studio.posterKey(shot), poster, stripH, dpr);
					const tw = one.width / dpr;
					g.globalAlpha = shot.state === 'changed' ? 0.4 : shot.state === 'making' || shot.state === 'queued' ? 0.55 : 1;
					for (let tx = x0 + Math.max(0, Math.floor((0 - x0) / tw)) * tw; tx < Math.min(x1, width); tx += tw) g.drawImage(one, tx, stripY, tw, stripH);
					g.globalAlpha = 1;
					// Frame lines between the pictures.
					g.fillStyle = 'rgba(0,0,0,0.55)';
					for (let tx = x0 + Math.max(1, Math.floor((0 - x0) / tw)) * tw; tx < Math.min(x1, width); tx += tw) g.fillRect(Math.round(tx), stripY, 1, stripH);
				} else if (!p.made) {
					// Not made: an empty frame with fine diagonal lines.
					g.strokeStyle = shot.state === 'failed' ? 'rgba(255,255,255,0.10)' : 'rgba(255,255,255,0.06)';
					g.lineWidth = 1;
					g.beginPath();
					for (let d = Math.max(x0, 0) - stripH; d < Math.min(x1, width); d += 7) {
						g.moveTo(d, stripY + stripH);
						g.lineTo(d + stripH, stripY);
					}
					g.stroke();
				}
				if (shot.state === 'making') {
					// How far the clip is, with a light band that moves while it is being made.
					const done = progress && progress.shot === shot.id && progress.steps ? Math.min(1, (progress.step ?? 0) / progress.steps) : 0;
					g.fillStyle = 'rgba(120,180,255,0.30)';
					g.fillRect(x0, stripY, w * done, stripH);
					const sweep = ((performance.now() / 1400) % 1) * (w + 60) - 60;
					const shine = g.createLinearGradient(x0 + sweep, 0, x0 + sweep + 60, 0);
					shine.addColorStop(0, 'rgba(255,255,255,0)');
					shine.addColorStop(0.5, 'rgba(255,255,255,0.16)');
					shine.addColorStop(1, 'rgba(255,255,255,0)');
					g.fillStyle = shine;
					g.fillRect(x0, stripY, w, stripH);
					g.fillStyle = LOOK.accent;
					g.fillRect(x0, y + h - 2, w * done, 2);
				}
				// The label bar: number, first words, and a take badge.
				g.fillStyle = darker(color, on ? 0.2 : 0.42);
				g.fillRect(x0, y, w, LABEL);
				g.fillStyle = 'rgba(255,255,255,0.14)';
				g.fillRect(x0, y, w, 1);
				const takes = shot.takes?.length ?? 0;
				const badge = takes > 1 && w > 70 ? `${(shot.takes ?? []).findIndex((t) => t.n === shot.take) + 1}/${takes}` : '';
				const number = String(p.index + 1);
				g.font = `600 10.5px ${fonts.data}`;
				const nw = g.measureText(number).width;
				if (w > nw + 8) {
					g.fillStyle = on ? '#ffffff' : 'rgba(255,255,255,0.92)';
					g.textBaseline = 'alphabetic';
					g.fillText(number, x0 + 5, y + 10.5);
				}
				const bw = badge ? g.measureText(badge).width + 8 : 0;
				label(g, firstWords(shot.prompt) || 'No description', x0 + nw + 11, y + 10.5, w - nw - 15 - bw, shot.prompt.trim() ? 'rgba(255,255,255,0.82)' : 'rgba(255,255,255,0.45)');
				if (badge) {
					g.fillStyle = 'rgba(0,0,0,0.45)';
					box(g, x1 - INSET - bw - 2, y + 2, bw, LABEL - 4, 2);
					g.fill();
					g.font = `500 9.5px ${fonts.data}`;
					g.fillStyle = 'rgba(255,255,255,0.85)';
					g.fillText(badge, x1 - INSET - bw + 2, y + 10.5);
				}
				// Trimmed ends carry a small light bar.
				g.fillStyle = 'rgba(255,255,255,0.55)';
				if ((shot.trim_start ?? 0) > 0 && p.made) g.fillRect(x0, stripY + 3, 2, stripH - 6);
				if ((shot.trim_end ?? 0) > 0 && p.made) g.fillRect(x1 - INSET - 2, stripY + 3, 2, stripH - 6);
				// The join into this shot.
				if (p.join === 'dissolve' && p.overlap > 0) {
					const ow = p.overlap * z.pps;
					g.fillStyle = 'rgba(8,9,11,0.5)';
					g.fillRect(x0, stripY, ow, stripH);
					g.strokeStyle = 'rgba(255,255,255,0.7)';
					g.lineWidth = 1;
					g.beginPath();
					g.moveTo(x0, stripY + stripH);
					g.lineTo(x0 + ow, stripY);
					g.moveTo(x0, stripY);
					g.lineTo(x0 + ow, stripY + stripH);
					g.stroke();
				} else if (p.fadeIn > 0) {
					g.fillStyle = 'rgba(0,0,0,0.78)';
					g.beginPath();
					g.moveTo(x0, stripY);
					g.lineTo(x0 + p.fadeIn * z.pps, stripY);
					g.lineTo(x0, stripY + stripH);
					g.fill();
				}
				if (p.fadeOut > 0) {
					g.fillStyle = 'rgba(0,0,0,0.78)';
					g.beginPath();
					g.moveTo(x1, stripY);
					g.lineTo(x1 - p.fadeOut * z.pps, stripY);
					g.lineTo(x1, stripY + stripH);
					g.fill();
				}
				g.restore();
				if (on) {
					box(g, x0 + 0.75, y + 0.75, w - 1.5, h - 1.5, 2.5);
					g.strokeStyle = '#ffffff';
					g.lineWidth = 1.5;
					g.stroke();
				}
			}
			if (sound && p.made) {
				const y = sound.top - scrollY + 2;
				const h = sound.height - 5;
				const quiet = shot.mute || !studio.heard[SHOTS];
				body(g, x0, y, w, h, quiet ? '#2c2e34' : '#1f4f5c', on);
				const peaks = studio.peaks.get(studio.peaksKey(shot));
				if (peaks) {
					g.save();
					box(g, x0, y, w, h);
					g.clip();
					g.fillStyle = quiet ? 'rgba(255,255,255,0.22)' : 'rgba(170,232,240,0.78)';
					wave(g, peaks.pairs, peaks.rate, p.from, x0, x1 - INSET, z.pps, y + h / 2, h / 2 - 2, shot.mute ? 0.25 : (shot.volume ?? 1));
					g.restore();
				}
				// The gain line.
				const gy = Math.round(sound.top - scrollY + gainY(sound, shot.mute ? 0 : (shot.volume ?? 1))) + 0.5;
				g.strokeStyle = shot.mute ? 'rgba(255,255,255,0.3)' : '#f0d27a';
				g.lineWidth = 1;
				g.beginPath();
				g.moveTo(Math.max(0, x0 + 1), gy);
				g.lineTo(Math.min(width, x1 - INSET - 1), gy);
				g.stroke();
				if (shot.mute && w > 34) label(g, 'Muted', x0 + 5, y + 11, w - 8, 'rgba(255,255,255,0.6)');
			}
		}
		// Join handles and link marks, over the regions.
		if (film) {
			const y = film.top - scrollY + 2;
			for (let i = 1; i < parts.length; i++) {
				const p = parts[i];
				const jx = Math.round(timeToX(p.start + p.overlap / 2, z));
				if (jx < -10 || jx > width + 10) continue;
				const shot = studio.shots[p.index];
				if (shot?.continues) {
					// Two small links: this shot carries on from the one before.
					g.strokeStyle = 'rgba(255,255,255,0.85)';
					g.lineWidth = 1.3;
					box(g, jx - 6.5, y + 4, 7, 5.5, 2.75);
					g.stroke();
					box(g, jx - 0.5, y + 4, 7, 5.5, 2.75);
					g.stroke();
				} else {
					const hot = hoverJoin === p.shot || (drag?.kind === 'join' && drag.id === p.shot);
					g.fillStyle = hot ? '#ffffff' : p.join === 'cut' ? 'rgba(255,255,255,0.42)' : LOOK.cycle;
					g.beginPath();
					g.moveTo(jx, y + 2);
					g.lineTo(jx + 4.5, y + 7);
					g.lineTo(jx, y + 12);
					g.lineTo(jx - 4.5, y + 7);
					g.closePath();
					g.fill();
					g.strokeStyle = 'rgba(0,0,0,0.6)';
					g.lineWidth = 1;
					g.stroke();
				}
			}
		}

		// Sound tracks.
		for (const row of rows) {
			if (row.kind !== 'track' || !row.track) continue;
			const y = row.top - scrollY + 2;
			const h = row.height - 5;
			if (y > height || y + h < 0) continue;
			const quiet = !studio.heard[row.id];
			for (const r of row.track.regions) {
				const x0 = timeToX(r.start, z);
				const x1 = timeToX(regionEnd(r), z);
				if (x1 < -2 || x0 > width + 2) continue;
				const on = studio.regionsSel.includes(r.id);
				const w = Math.max(2, x1 - x0 - INSET);
				const color = quiet ? '#3a3c43' : row.track.color;
				const peaks = studio.peaks.get(`file:${r.file}`);
				const lost = peaks !== undefined && peaks.seconds <= 0;
				body(g, x0, y, w, h, lost ? '#5a2f2c' : color, on);
				g.save();
				box(g, x0, y, w, h);
				g.clip();
				g.fillStyle = darker(lost ? '#5a2f2c' : color, on ? 0.25 : 0.45);
				g.fillRect(x0, y, w, LABEL);
				g.fillStyle = 'rgba(255,255,255,0.14)';
				g.fillRect(x0, y, w, 1);
				const unit = r.length * z.pps;
				if (peaks && peaks.pairs.length > 0) {
					g.fillStyle = quiet ? 'rgba(255,255,255,0.25)' : on ? 'rgba(6,8,12,0.78)' : 'rgba(6,8,12,0.62)';
					for (let k = 0; k < Math.max(1, r.loops); k++) {
						const ux = x0 + k * unit;
						if (ux > width) break;
						if (ux + unit < 0) continue;
						if (k > 0) g.fillStyle = quiet ? 'rgba(255,255,255,0.16)' : 'rgba(6,8,12,0.38)';
						wave(g, peaks.pairs, peaks.rate, r.offset, ux, Math.min(ux + unit, x1 - INSET), z.pps, y + LABEL + (h - LABEL) / 2, (h - LABEL) / 2 - 2, Math.min(2, r.gain));
					}
				}
				// Each repeat starts at a notch.
				g.fillStyle = 'rgba(0,0,0,0.5)';
				for (let k = 1; k < r.loops; k++) g.fillRect(Math.round(x0 + k * unit), y, 1, h);
				// Fades: the corner that is faded away is darkened under a line.
				g.fillStyle = 'rgba(8,9,11,0.5)';
				g.strokeStyle = 'rgba(255,255,255,0.8)';
				g.lineWidth = 1;
				if (r.fade_in > 0) {
					const fx = x0 + r.fade_in * z.pps;
					g.beginPath();
					g.moveTo(x0, y + LABEL);
					g.lineTo(fx, y + LABEL);
					g.lineTo(x0, y + h);
					g.fill();
					g.beginPath();
					g.moveTo(x0, y + h);
					g.lineTo(fx, y + LABEL);
					g.stroke();
				}
				if (r.fade_out > 0) {
					const fx = x1 - INSET - r.fade_out * z.pps;
					g.beginPath();
					g.moveTo(x1, y + LABEL);
					g.lineTo(fx, y + LABEL);
					g.lineTo(x1, y + h);
					g.fill();
					g.beginPath();
					g.moveTo(x1 - INSET, y + h);
					g.lineTo(fx, y + LABEL);
					g.stroke();
				}
				const name = fileName(r.file).replace(/\.[^.]+$/, '');
				const extra = `${r.loops > 1 ? `  ×${r.loops}` : ''}${Math.abs(r.gain - 1) > 0.005 ? `  ${dbLabel(r.gain)} dB` : ''}${lost ? '  file not found' : ''}`;
				label(g, name + extra, x0 + 5, y + 10.5, w - 9, on ? '#ffffff' : 'rgba(255,255,255,0.88)');
				g.restore();
				// A thin warm line on regions that lie over another one on the same track.
				if (studio.overlaps.has(r.id)) {
					g.fillStyle = LOOK.cycle;
					g.fillRect(x0 + 2, y + h - 2, w - 4, 1.5);
				}
				if (on || hoverRegion === r.id) {
					// The fade handles, at the top corners.
					for (const fx of [x0 + Math.max(r.fade_in * z.pps, 0), x1 - INSET - Math.max(r.fade_out * z.pps, 0)]) {
						if (w < 30) break;
						g.beginPath();
						g.arc(Math.min(x1 - INSET - 4, Math.max(x0 + 4, fx)), y + LABEL + 1, 3, 0, Math.PI * 2);
						g.fillStyle = '#ffffff';
						g.fill();
						g.strokeStyle = 'rgba(0,0,0,0.7)';
						g.stroke();
					}
				}
				if (on) {
					box(g, x0 + 0.75, y + 0.75, w - 1.5, h - 1.5, 2.5);
					g.strokeStyle = '#ffffff';
					g.lineWidth = 1.5;
					g.stroke();
				}
			}
		}

		// The cycle range, lightly, down the lanes.
		if (studio.cycle.to > studio.cycle.from) {
			const cx0 = timeToX(studio.cycle.from, z);
			const cx1 = timeToX(studio.cycle.to, z);
			g.fillStyle = alpha(LOOK.cycle, studio.cycle.on ? 0.07 : 0.025);
			g.fillRect(cx0, 0, cx1 - cx0, Math.min(height, rowsEnd - scrollY));
		}

		// What is in hand: where dragged shots would land, with an outline of what moves.
		if (drag?.kind === 'shots' && drag.moved && drag.place !== undefined && film) {
			const at = drag.place >= parts.length ? studio.plan.total : parts[drag.place].start;
			const ix = Math.round(timeToX(at, z));
			const y = film.top - scrollY;
			g.fillStyle = LOOK.accent;
			g.fillRect(ix - 1, y, 3, film.height - 1);
			const grabbed = parts.find((q) => q.shot === drag!.id);
			if (grabbed) {
				const gw = grabbed.length * z.pps;
				const gx = drag.x - (drag.t0 - grabbed.start) * z.pps;
				g.setLineDash([4, 3]);
				g.strokeStyle = 'rgba(255,255,255,0.85)';
				g.lineWidth = 1;
				box(g, gx + 0.5, y + 2.5, gw - 1, film.height - 6);
				g.stroke();
				g.setLineDash([]);
				g.fillStyle = 'rgba(255,255,255,0.08)';
				g.fill();
			}
		}
		// Where moved regions came from.
		if (drag?.kind === 'regions' && drag.moved && drag.regions && !drag.copy) {
			const firstTrack = rows.findIndex((r) => r.kind === 'track');
			g.setLineDash([4, 3]);
			g.strokeStyle = 'rgba(255,255,255,0.45)';
			g.lineWidth = 1;
			for (const o of drag.regions.values()) {
				const row = rows[firstTrack + o.row];
				if (!row) continue;
				box(g, timeToX(o.region.start, z) + 0.5, row.top - scrollY + 2.5, regionSpan(o.region) * z.pps - 1, row.height - 6);
				g.stroke();
			}
			g.setLineDash([]);
		}
		if (snapAt !== null) {
			g.fillStyle = LOOK.cycle;
			g.fillRect(Math.round(timeToX(snapAt, z)), 0, 1, height);
		}
		if (dropAt) {
			const row = rowOf(dropAt.row);
			const dx = Math.round(timeToX(dropAt.time, z));
			g.fillStyle = LOOK.accent;
			g.fillRect(dx - 1, row ? row.top - scrollY : rowsEnd - scrollY, 2, row ? row.height : 40);
			if (!row) {
				g.fillStyle = alpha(LOOK.accent, 0.12);
				g.fillRect(0, rowsEnd - scrollY, width, 40);
			}
		}
	}

	function drawRuler() {
		const el = ruler;
		if (!el || width <= 0) return;
		const g = el.getContext('2d')!;
		fit(g, width, RULER, el);
		const z = zoom();
		g.fillStyle = LOOK.raised;
		g.fillRect(0, 0, width, RULER);
		g.fillStyle = LOOK.panel;
		g.fillRect(0, 0, width, CYCLE);
		// The cycle range: a coloured strip along the top.
		if (studio.cycle.to > studio.cycle.from) {
			const x0 = timeToX(studio.cycle.from, z);
			const x1 = timeToX(studio.cycle.to, z);
			g.fillStyle = studio.cycle.on ? LOOK.cycle : alpha(LOOK.cycle, 0.28);
			box(g, x0, 1.5, Math.max(3, x1 - x0), CYCLE - 3, 2);
			g.fill();
		}
		const steps = rulerSteps(z.pps);
		const from = Math.max(0, Math.floor(xToTime(0, z) / steps.major) * steps.major);
		const to = xToTime(width, z);
		g.font = `500 10px ${fonts.data}`;
		g.textBaseline = 'alphabetic';
		const each = Math.round(steps.major / steps.minor);
		for (let n = Math.round(from / steps.minor); n * steps.minor <= to; n++) {
			const t = n * steps.minor;
			const x = Math.round(timeToX(t, z));
			const major = n % each === 0;
			g.fillStyle = major ? LOOK.faint : LOOK.line;
			g.fillRect(x, major ? CYCLE : RULER - 5, 1, major ? RULER - CYCLE : 5);
			if (major) {
				g.fillStyle = LOOK.muted;
				g.fillText(rulerLabel(t, steps.frames), x + 4, CYCLE + 11);
			}
		}
		// The end of the film.
		if (studio.plan.total > 0) {
			const ex = Math.round(timeToX(studio.plan.total, z));
			g.fillStyle = LOOK.ink;
			g.fillRect(ex, CYCLE, 1, RULER - CYCLE);
			g.fillRect(ex - 3, RULER - 3, 3, 3);
		}
		g.fillStyle = LOOK.edge;
		g.fillRect(0, RULER - 1, width, 1);
	}

	// Everything the lanes show is read here, so they are drawn again when any of it changes.
	$effect(() => {
		void studio.drawn;
		void studio.pps;
		void width;
		void height;
		draw();
	});
	$effect(() => {
		void studio.pps;
		void studio.cycle.on;
		void studio.cycle.from;
		void studio.cycle.to;
		void studio.plan.total;
		void width;
		drawRuler();
		place();
	});
	$effect(() => {
		void studio.jumps;
		place();
	});

	// While a shot is being made, its region moves.
	let shimmer: ReturnType<typeof setInterval> | undefined;
	$effect(() => {
		const making = studio.shots.some((s) => s.state === 'making');
		clearInterval(shimmer);
		if (making) shimmer = setInterval(draw, 90);
	});

	// --- what is dropped from the Library -------------------------------------------------------

	const dropKind = (e: DragEvent) => {
		const types = [...(e.dataTransfer?.types ?? [])];
		if (types.includes('application/x-rigbrew-sound')) return 'sound';
		const take = types.find((t) => t.startsWith('application/x-rigbrew-take-'));
		return take ? take.slice('application/x-rigbrew-take-'.length) : '';
	};

	function dragover(e: DragEvent) {
		if (!canvas) return;
		const kind = dropKind(e);
		if (!kind) return;
		const { x, y } = local(e, canvas);
		const hit = hitTest(x, y);
		if (kind === 'sound') {
			e.preventDefault();
			const row = rowAt(y + scrollY);
			dropAt = { row: row?.kind === 'track' ? row.id : '', time: Math.max(0, snapTime(xToTime(x, zoom()), marks(), studio.pps, studio.layout.snap && !e.altKey).time) };
		} else if (hit.what === 'shot' && hit.id === kind) {
			// A take goes on the shot it is a take of.
			e.preventDefault();
			if (!studio.shotsSel.includes(kind)) studio.selectShot(kind);
			dropAt = null;
		} else dropAt = null;
		draw();
	}

	function drop(e: DragEvent) {
		const kind = dropKind(e);
		const at = dropAt;
		dropAt = null;
		if (!kind) return;
		e.preventDefault();
		if (kind === 'sound' && at) studio.addRegion(at.row, e.dataTransfer?.getData('application/x-rigbrew-sound') ?? '', at.time);
		else if (kind !== 'sound') {
			const n = Number(e.dataTransfer?.getData(`application/x-rigbrew-take-${kind}`));
			if (n > 0) studio.useTake(kind, n);
		}
		draw();
	}

	// --- headers ------------------------------------------------------------------------------------

	let renaming = $state('');
	let renameText = $state('');
	function rename(id: string) {
		const name = renameText.trim().slice(0, 40);
		renaming = '';
		if (!name || name === studio.trackOf(id)?.name) return;
		studio.edit('Rename track', () => {
			const t = studio.trackOf(id);
			if (t) t.name = name;
		});
	}

	function recolor(id: string) {
		studio.edit('Track colour', () => {
			const t = studio.trackOf(id);
			if (t) t.color = TRACK_COLORS[(TRACK_COLORS.indexOf(t.color) + 1) % TRACK_COLORS.length];
		});
	}

	const strip = (id: string) => (id === SHOTS ? studio.doc!.shot_track : studio.trackOf(id)!);
	const setStrip = (id: string, what: 'volume' | 'pan', value: number) => {
		const s = strip(id);
		if (s) s[what] = value;
	};

	// Dragging a header up or down puts the track at another place.
	let lift = $state<{ id: string; to: number; y: number } | null>(null);
	function headDown(e: PointerEvent, id: string) {
		if (e.button !== 0 || (e.target as HTMLElement).closest('button, input, [role=slider]')) return;
		studio.selectTrack(id);
		const row = rowOf(id);
		if (!row || row.kind !== 'track') return;
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		const y0 = e.clientY;
		const onMove = (m: PointerEvent) => {
			if (Math.abs(m.clientY - y0) < 5 && !lift) return;
			const tracks = rows.filter((r) => r.kind === 'track');
			const cy = row.top + row.height / 2 + (m.clientY - y0);
			let to = tracks.findIndex((r) => cy < r.top + r.height / 2);
			if (to < 0) to = tracks.length;
			lift = { id, to, y: to >= tracks.length ? tracks[tracks.length - 1].top + tracks[tracks.length - 1].height : tracks[to].top };
		};
		const onUp = () => {
			el.removeEventListener('pointermove', onMove);
			el.removeEventListener('pointerup', onUp);
			el.removeEventListener('pointercancel', onUp);
			if (lift) studio.moveTrack(lift.id, lift.to);
			lift = null;
		};
		el.addEventListener('pointermove', onMove);
		el.addEventListener('pointerup', onUp);
		el.addEventListener('pointercancel', onUp);
	}

	// Dragging the edge between the headers and the lanes makes the headers wider or narrower.
	function headWidth(e: PointerEvent) {
		const el = e.currentTarget as HTMLElement;
		el.setPointerCapture(e.pointerId);
		const x0 = e.clientX;
		const w0 = studio.layout.headWidth;
		const onMove = (m: PointerEvent) => studio.set('headWidth', Math.min(340, Math.max(170, w0 + m.clientX - x0)));
		const onUp = () => {
			el.removeEventListener('pointermove', onMove);
			el.removeEventListener('pointerup', onUp);
		};
		el.addEventListener('pointermove', onMove);
		el.addEventListener('pointerup', onUp);
	}

	let observer: ResizeObserver | undefined;
	onMount(() => {
		if (root) {
			const style = getComputedStyle(root);
			fonts = { text: style.fontFamily || 'sans-serif', data: style.getPropertyValue('--rb-data').trim() || 'monospace' };
		}
		observer = new ResizeObserver(() => {
			if (!lanes) return;
			width = lanes.clientWidth;
			height = lanes.clientHeight;
		});
		if (lanes) observer.observe(lanes);
		// Fonts arrive after the first drawing.
		document.fonts?.ready.then(() => (draw(), drawRuler()));
		tick().then(scrolled);
	});

	onDestroy(() => {
		stopFrames();
		observer?.disconnect();
		clearInterval(shimmer);
		cancelAnimationFrame(autoFrame);
	});
</script>

<svelte:window onkeydowncapture={escape} />

<div class="tracks" bind:this={root} style:--head="{studio.layout.headWidth}px">
	<div class="corner">
		<button class="tb" onclick={() => studio.addTrack()} title="Add a sound track"><StudioIcon name="plus" size={12} /> Track</button>
		<button class="tb" class:on={studio.layout.snap} aria-pressed={studio.layout.snap} onclick={() => studio.set('snap', !studio.layout.snap)} title="Snap: edges land on frames, on other edges and on the playhead. Hold Alt to place freely (Ctrl while copying).">
			<StudioIcon name="snap" size={12} /> Snap
		</button>
		<span class="grow"></span>
		<input class="vz" type="range" min="0.6" max="2" step="0.05" value={vz} oninput={(e) => studio.set('vzoom', Number(e.currentTarget.value))} aria-label="Height of the tracks" title="Height of the tracks" />
	</div>
	<div class="ruler">
		<canvas bind:this={ruler} style:width="{width}px" style:height="{RULER}px" onpointerdown={rulerDown} onpointermove={rulerMove} onpointerup={rulerUp} onpointercancel={rulerUp} aria-label="Time ruler. Click or drag to move the playhead; drag in its upper edge to set the cycle range."></canvas>
		<div class="playcap" bind:this={playcap} aria-hidden="true"></div>
	</div>
	<div class="heads">
		<div class="inner" bind:this={headsInner}>
			{#each rows as row (row.id)}
				{@const on = studio.track === row.id}
				<!-- svelte-ignore a11y_no_static_element_interactions -->
				<div class="head {row.kind}" class:on class:picked={on && studio.trackSel} class:small={row.height < 50} style:height="{row.height}px" onpointerdown={(e) => headDown(e, row.id)}>
					{#if row.kind === 'film'}
						<span class="chip film"><StudioIcon name="film" size={12} /></span>
						<div class="col">
							<span class="tname">Film</span>
							<span class="tmeta">{studio.doc?.shots.length ?? 0} {studio.doc?.shots.length === 1 ? 'shot' : 'shots'}, {studio.shots.filter((s) => s.state === 'done').length} made</span>
						</div>
						<button class="mini" onclick={() => studio.addShot()} title="Add a shot at the end" aria-label="Add a shot"><StudioIcon name="plus" size={12} /></button>
					{:else}
						{@const s = strip(row.id)}
						{#if row.kind === 'shots'}
							<span class="chip shots"><StudioIcon name="wave" size={12} /></span>
						{:else}
							<button class="chip" style:background={row.track?.color} onclick={() => recolor(row.id)} title="Colour of the track. Click for the next one." aria-label="Colour of {row.track?.name}"></button>
						{/if}
						<div class="col">
							<div class="line">
								{#if renaming === row.id}
									<!-- svelte-ignore a11y_autofocus -->
									<input class="rename" bind:value={renameText} autofocus onblur={() => rename(row.id)} onkeydown={(e) => (e.key === 'Enter' ? rename(row.id) : e.key === 'Escape' ? (renaming = '') : null, e.stopPropagation())} aria-label="Name of the track" />
								{:else if row.kind === 'shots'}
									<span class="tname">Shot sound</span>
								{:else}
									<!-- svelte-ignore a11y_no_static_element_interactions -->
									<span class="tname" ondblclick={() => ((renaming = row.id), (renameText = row.track?.name ?? ''))} title="Double-click to rename">{row.track?.name}</span>
								{/if}
								<button class="ms m" class:lit={s.mute} aria-pressed={s.mute} onclick={() => studio.toggle(row.id, 'mute')} title="Mute (M)" aria-label="Mute {row.track?.name ?? 'shot sound'}">M</button>
								<button class="ms s" class:lit={s.solo} aria-pressed={s.solo} onclick={() => studio.toggle(row.id, 'solo')} title="Solo (S)" aria-label="Solo {row.track?.name ?? 'shot sound'}">S</button>
							</div>
							<div class="line">
								<StudioFader
									value={s.volume}
									label="Volume of {row.track?.name ?? 'the shot sound'}"
									onstart={() => studio.begin()}
									onchange={(v) => studio.live(() => setStrip(row.id, 'volume', v))}
									onend={() => studio.commit('Volume')}
									onset={(v) => studio.edit('Volume', () => setStrip(row.id, 'volume', v), `vol:${row.id}`)}
								/>
								<StudioKnob
									value={s.pan}
									size={18}
									label="Pan of {row.track?.name ?? 'the shot sound'}"
									onstart={() => studio.begin()}
									onchange={(v) => studio.live(() => setStrip(row.id, 'pan', v))}
									onend={() => studio.commit('Pan')}
									onset={(v) => studio.edit('Pan', () => setStrip(row.id, 'pan', v), `pan:${row.id}`)}
								/>
								<StudioMeter id={row.id} length={34} thick={7} />
							</div>
						</div>
						{#if row.kind === 'track'}
							<div class="acts">
								<button class="mini" onclick={() => studio.duplicateTrack(row.id)} title="Duplicate this track" aria-label="Duplicate {row.track?.name}"><StudioIcon name="dup" size={11} /></button>
								<button class="mini" onclick={() => studio.deleteTrack(row.id)} title="Delete this track" aria-label="Delete {row.track?.name}"><StudioIcon name="trash" size={11} /></button>
							</div>
						{/if}
					{/if}
				</div>
			{/each}
			{#if lift}<div class="lift" style:top="{lift.y - 1}px"></div>{/if}
			<div class="more">
				<button class="tb" onclick={() => studio.addTrack()}><StudioIcon name="plus" size={12} /> Add a sound track</button>
			</div>
		</div>
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div class="headedge" onpointerdown={headWidth}></div>
	</div>
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div class="lanes" bind:this={lanes} onscroll={scrolled} onwheel={wheel}>
		<div class="stick" style:width="{width}px" style:height="{height}px">
			<canvas
				bind:this={canvas}
				class:busy={dragging !== ''}
				style:width="{width}px"
				style:height="{height}px"
				onpointerdown={down}
				onpointermove={move}
				onpointerup={up}
				onpointercancel={up}
				ondblclick={dblclick}
				ondragover={dragover}
				ondragleave={() => ((dropAt = null), draw())}
				ondrop={drop}
				aria-label="The tracks: the film's shots, their sound, and the sound tracks"
			></canvas>
			{#if band}<div class="band" style:left="{band.x}px" style:top="{band.y}px" style:width="{band.w}px" style:height="{band.h}px"></div>{/if}
			<div class="playhead" bind:this={playhead} aria-hidden="true"></div>
			{#if studio.doc && studio.doc.shots.length === 0}
				<p class="hint" style:top="{(rows[0]?.height ?? 78) / 2 - 8}px">Double-click here to add a shot, or write the shots from an idea in the Inspector.</p>
			{/if}
		</div>
		<div class="spacer" style:width="{contentWidth}px" style:height="{contentHeight}px" style:margin-top="{-height}px"></div>
	</div>
	{#if tip}<div class="tip" style:left="{tip.x}px" style:top="{tip.y}px">{tip.text}</div>{/if}
</div>

<style>
	.tracks {
		position: relative;
		flex: 1 1 0;
		min-height: 120px;
		display: grid;
		grid-template-columns: var(--head) minmax(0, 1fr);
		grid-template-rows: 28px minmax(0, 1fr);
		background: var(--st-bg);
	}

	.corner {
		display: flex;
		align-items: center;
		gap: 4px;
		padding: 0 6px;
		background: var(--st-raised);
		border-right: 1px solid var(--st-edge);
		border-bottom: 1px solid var(--st-edge);
	}

	.grow {
		flex: 1;
	}

	.tb {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		height: 20px;
		padding: 0 6px;
		border-radius: 3px;
		font-size: 11px;
		color: var(--st-muted);
		background: rgba(255, 255, 255, 0.045);
		box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.05);
	}

	.tb:hover {
		color: var(--st-ink);
		background: rgba(255, 255, 255, 0.09);
	}

	.tb.on {
		color: #10131a;
		background: var(--st-accent);
	}

	.vz {
		width: 46px;
		height: 12px;
		accent-color: var(--st-muted);
	}

	.ruler {
		position: relative;
		overflow: hidden;
		background: var(--st-raised);
	}

	.ruler canvas {
		display: block;
		touch-action: none;
	}

	/* The head of the playhead, in the ruler. */
	.playcap {
		position: absolute;
		left: -5.5px;
		top: 12px;
		width: 11px;
		height: 15px;
		pointer-events: none;
		background: var(--st-playhead);
		clip-path: polygon(0 0, 100% 0, 100% 55%, 50% 100%, 0 55%);
		will-change: transform;
	}

	.heads {
		position: relative;
		overflow: hidden;
		background: var(--st-panel);
		border-right: 1px solid var(--st-edge);
	}

	.inner {
		position: relative;
		will-change: transform;
	}

	.headedge {
		position: absolute;
		top: 0;
		right: -1px;
		bottom: 0;
		width: 5px;
		cursor: col-resize;
	}

	.head {
		position: relative;
		display: flex;
		align-items: center;
		gap: 7px;
		padding: 0 6px 1px 7px;
		border-bottom: 1px solid var(--st-edge);
		background: var(--st-raised);
		box-shadow: inset 0 1px 0 rgba(255, 255, 255, 0.035);
	}

	.head.on {
		background: #262930;
	}

	.head.picked {
		box-shadow:
			inset 2px 0 0 var(--st-accent),
			inset 0 1px 0 rgba(255, 255, 255, 0.035);
	}

	.chip {
		flex: none;
		width: 18px;
		height: 18px;
		display: grid;
		place-items: center;
		border-radius: 3px;
		color: rgba(255, 255, 255, 0.9);
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.35);
	}

	.chip.film {
		background: #2a7688;
	}

	.chip.shots {
		background: #1f4f5c;
	}

	.col {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}

	.line {
		display: flex;
		align-items: center;
		gap: 5px;
		min-width: 0;
	}

	.small .col {
		gap: 1px;
	}

	.tname {
		flex: 1;
		min-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
		font-size: 11.5px;
		font-weight: 600;
		color: var(--st-ink);
	}

	.tmeta {
		font-size: 10.5px;
		color: var(--st-faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}

	.rename {
		flex: 1;
		min-width: 0;
		height: 18px;
		padding: 0 4px;
		border-radius: 2px;
		border: 1px solid var(--st-accent);
		background: var(--st-edge);
		color: var(--st-ink);
		font: 600 11.5px/1 var(--rb-text);
		outline: none;
	}

	.ms {
		flex: none;
		width: 18px;
		height: 16px;
		border-radius: 2.5px;
		font: 700 9.5px/16px var(--rb-text);
		text-align: center;
		color: var(--st-muted);
		background: rgba(255, 255, 255, 0.06);
		box-shadow: inset 0 0 0 1px rgba(0, 0, 0, 0.4);
	}

	.ms:hover {
		color: var(--st-ink);
		background: rgba(255, 255, 255, 0.12);
	}

	.ms.m.lit {
		color: #0c1620;
		background: #58b4f4;
	}

	.ms.s.lit {
		color: #201a08;
		background: #f0c948;
	}

	.acts {
		position: absolute;
		right: 4px;
		bottom: 2px;
		display: none;
		gap: 1px;
		background: inherit;
	}

	.head:hover .acts,
	.head.picked .acts {
		display: flex;
	}

	.mini {
		flex: none;
		width: 18px;
		height: 16px;
		display: grid;
		place-items: center;
		border-radius: 2.5px;
		color: var(--st-faint);
	}

	.mini:hover {
		color: var(--st-ink);
		background: rgba(255, 255, 255, 0.1);
	}

	.lift {
		position: absolute;
		left: 0;
		right: 0;
		height: 2px;
		background: var(--st-accent);
	}

	.more {
		padding: 8px 7px;
	}

	.lanes {
		position: relative;
		overflow: auto;
		background: var(--st-bg);
		scrollbar-width: thin;
		scrollbar-color: #3a3d45 var(--st-bg);
		overscroll-behavior: contain;
	}

	.stick {
		position: sticky;
		left: 0;
		top: 0;
		overflow: hidden;
	}

	.stick canvas {
		display: block;
		touch-action: none;
	}

	.spacer {
		pointer-events: none;
	}

	.playhead {
		position: absolute;
		left: 0;
		top: 0;
		bottom: 0;
		width: 1px;
		background: var(--st-playhead);
		box-shadow: 0 0 0 0.5px rgba(0, 0, 0, 0.55);
		pointer-events: none;
		will-change: transform;
	}

	.band {
		position: absolute;
		border: 1px solid var(--st-accent);
		background: color-mix(in srgb, var(--st-accent) 14%, transparent);
		pointer-events: none;
	}

	.hint {
		position: absolute;
		left: 14px;
		font-size: 11.5px;
		color: var(--st-faint);
		pointer-events: none;
	}

	.tip {
		position: fixed;
		z-index: 40;
		padding: 3px 7px;
		border-radius: 3px;
		background: #000;
		color: #fff;
		font: 500 10.5px/1.3 var(--rb-data);
		font-variant-numeric: tabular-nums;
		white-space: nowrap;
		pointer-events: none;
		box-shadow: 0 2px 8px rgba(0, 0, 0, 0.5);
	}
</style>
