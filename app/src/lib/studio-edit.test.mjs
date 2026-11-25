// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { filmPlan, partLight } from './studio.ts';
import {
	History,
	balance,
	dbLabel,
	dbOf,
	captionRows,
	cueAt,
	docOf,
	duckLevel,
	names,
	told,
	dropPlace,
	fadeLevel,
	faderToGain,
	gainOf,
	gainToFader,
	heard,
	joinFromDrag,
	meterFill,
	moveRegion,
	moveShots,
	newId,
	overlapping,
	panLabel,
	pasteRegions,
	regionEnd,
	saveBody,
	takeWork,
	splitRegion,
	stripGain,
	trimRegionEnd,
	trimRegionStart,
	trimShot
} from './studio-edit.ts';
import {
	MAX_PPS,
	MIN_PPS,
	edgeScroll,
	fitZoom,
	followScroll,
	parseTime,
	rulerLabel,
	rulerSteps,
	shortTime,
	sliderToZoom,
	snapTime,
	timeToX,
	timecode,
	xToTime,
	zoomAround,
	zoomSpanAround,
	zoomToSlider,
	zoomedSpan
} from './studio-time.ts';

const close = (a, b, by = 1e-9) => assert.ok(Math.abs(a - b) <= by, `${a} is not ${b}`);
const region = (id, start, length, over = {}) => ({ id, file: '/s/a.wav', start, offset: 0, length, gain: 1, fade_in: 0, fade_out: 0, loops: 1, ...over });

test('timecode counts hours, minutes, seconds and frames at 24 a second', () => {
	assert.equal(timecode(0), '00:00:00:00');
	assert.equal(timecode(1 / 24), '00:00:00:01');
	assert.equal(timecode(67 + 13 / 24), '00:01:07:13');
	assert.equal(timecode(3600 + 23 / 24), '01:00:00:23');
	// A moment a hair before a frame is that frame, and nothing is before the start.
	assert.equal(timecode(1 - 1e-7), '00:00:01:00');
	assert.equal(timecode(-3), '00:00:00:00');
	assert.equal(shortTime(2 + 13 / 24), '2:13');
	assert.equal(shortTime(62.5), '1:02:12');
	assert.equal(shortTime(-0.5), '-0:12');
	assert.equal(parseTime('7.5'), 7.5);
	assert.equal(parseTime('7:12'), 7.5);
	assert.equal(parseTime('1:07:12'), 67.5);
	assert.equal(parseTime('00:01:07:12'), 67.5);
	assert.equal(parseTime('2,25'), 2.25);
	for (const bad of ['', 'soon', '1:2:3:4:5', '1:x', '-3']) assert.equal(parseTime(bad), null, bad);
	assert.equal(rulerLabel(7, false), '0:07');
	assert.equal(rulerLabel(125.5, true), '2:05:12');
	assert.equal(rulerLabel(3725, false), '1:02:05');
});

test('seconds and pixels follow the zoom', () => {
	const z = { pps: 50, scroll: 100 };
	assert.equal(timeToX(4, z), 100);
	assert.equal(xToTime(100, z), 4);
	// Zooming keeps the moment under the pointer where it is.
	const closer = zoomAround(z, 100, 2);
	assert.equal(closer.pps, 100);
	assert.equal(xToTime(100, closer), 4);
	const far = { pps: 50, scroll: 1000 };
	const wider = zoomAround(far, 300, 0.25);
	close(xToTime(300, wider), xToTime(300, far));
	// The start of the film cannot be scrolled past, and the zoom has ends.
	assert.equal(zoomAround({ pps: 50, scroll: 0 }, 400, 0.5).scroll, 0);
	assert.equal(zoomAround(z, 0, 1e6).pps, MAX_PPS);
	assert.equal(zoomAround(z, 0, 1e-6).pps, MIN_PPS);
	// A film fits its window with a little air.
	const fit = fitZoom(60, 1000);
	assert.equal(fit.scroll, 0);
	close(fit.pps * 60, 960);
	assert.equal(fitZoom(0.1, 1000).pps, MAX_PPS);
	// The slider is even: its middle is the middle in factors, and it goes there and back.
	for (const pps of [MIN_PPS, 3, 48, MAX_PPS]) close(sliderToZoom(zoomToSlider(pps)), pps, 1e-6);
	close(sliderToZoom(0.5), Math.sqrt(MIN_PPS * MAX_PPS), 1e-6);
});

test('the ruler has marks a person can read at every zoom', () => {
	// As close as it goes, a frame is 24 pixels: a label every fourth frame, a mark on every frame.
	assert.deepEqual(rulerSteps(MAX_PPS), { major: 4 / 24, minor: 1 / 24, frames: true });
	// A second is 100 pixels: a label every second, a small mark every quarter.
	assert.deepEqual(rulerSteps(100), { major: 1, minor: 0.25, frames: false });
	// Five minutes on a screen: a label every 30 seconds, a small mark every 15.
	assert.deepEqual(rulerSteps(4), { major: 30, minor: 15, frames: false });
	// Labels never crowd, and small marks never blur.
	for (const pps of [0.25, 0.7, 3, 11, 40, 97, 300, 576]) {
		const s = rulerSteps(pps);
		assert.ok(s.major * pps >= 84 || s.major === 3600, `labels at ${pps}`);
		assert.ok(s.minor * pps >= 5, `marks at ${pps}`);
	}
});

test('a dragged edge snaps to region edges and the playhead, else to frames', () => {
	const marks = [2, 5.5, 9];
	// Within reach of a mark at this zoom.
	assert.deepEqual(snapTime(5.44, marks, 100), { time: 5.5, mark: 5.5 });
	// Out of reach: the nearest frame.
	assert.deepEqual(snapTime(5.3, marks, 100), { time: 127 / 24, mark: null });
	// Zoomed far in, the same distance is many pixels: no mark, a frame.
	assert.deepEqual(snapTime(5.44, marks, 576), { time: 131 / 24, mark: null });
	// The nearer of two marks wins.
	assert.equal(snapTime(2.03, [2, 2.05], 100).time, 2.05);
	// Snapping off (Alt held): where the pointer is, but never before the film.
	assert.deepEqual(snapTime(5.44, marks, 100, false), { time: 5.44, mark: null });
	assert.equal(snapTime(-1, marks, 100, false).time, 0);
	assert.equal(snapTime(-1, [], 100).time, 0);
});

test('the window follows the playhead and scrolls at its edges', () => {
	const z = { pps: 100, scroll: 0 };
	assert.equal(followScroll(4, z, 1000), 0);
	// Past the place it is held at, the window moves with the playhead: it stands still on the screen.
	assert.equal(followScroll(6.2, z, 1000), 0);
	assert.equal(followScroll(6.3, z, 1000), 10);
	assert.equal(followScroll(9.9, z, 1000), 990 - 620);
	assert.equal(followScroll(9.9, { pps: 100, scroll: 370 }, 1000), 370);
	// The Editor's own zoom: a part of what it shows, held inside it, the moment under the pointer staying put.
	const base = { from: 2, to: 10 };
	assert.deepEqual(zoomedSpan(base, 1, 5), { from: 2, to: 10 });
	assert.deepEqual(zoomedSpan(base, 4, 5), { from: 5, to: 7 });
	assert.deepEqual(zoomedSpan(base, 4, 99), { from: 8, to: 10 });
	assert.deepEqual(zoomedSpan(base, 0.2, -4), { from: 2, to: 10 });
	assert.deepEqual(zoomSpanAround(base, base, 2, 0.5), { zoom: 2, from: 4 });
	assert.deepEqual(zoomSpanAround(base, { from: 4, to: 8 }, 2, 0.25), { zoom: 4, from: 4.5 });
	assert.deepEqual(zoomSpanAround(base, { from: 4, to: 8 }, 0.1, 0.5), { zoom: 1, from: 2 });
	assert.equal(zoomSpanAround(base, base, 1000, 0.5).zoom, 64);
	// Behind the window (after a jump back): the same.
	assert.equal(followScroll(1, { pps: 100, scroll: 500 }, 1000), 0);
	assert.equal(edgeScroll(500, 1000), 0);
	assert.ok(edgeScroll(4, 1000) < edgeScroll(30, 1000) && edgeScroll(30, 1000) < 0);
	assert.ok(edgeScroll(999, 1000) > edgeScroll(970, 1000) && edgeScroll(970, 1000) > 0);
	assert.equal(edgeScroll(-50, 1000), -22);
});

test('moving shots ripples the others', () => {
	const film = ['a', 'b', 'c', 'd', 'e'].map((id) => ({ id }));
	const ids = (list) => list.map((s) => s.id).join('');
	// One shot to the right: in front of the shot that stood at that place.
	assert.equal(ids(moveShots(film, ['a'], 3)), 'bcade');
	assert.equal(ids(moveShots(film, ['a'], 5)), 'bcdea');
	assert.equal(ids(moveShots(film, ['d'], 0)), 'dabce');
	// Dropped on itself or right after itself: nothing changes.
	assert.equal(ids(moveShots(film, ['c'], 2)), 'abcde');
	assert.equal(ids(moveShots(film, ['c'], 3)), 'abcde');
	// Several shots go together, in the order they had, even when they were apart.
	assert.equal(ids(moveShots(film, ['d', 'a'], 3)), 'bcade');
	assert.equal(ids(moveShots(film, ['b', 'e'], 0)), 'beacd');
	assert.equal(ids(moveShots(film, ['x'], 0)), 'abcde');
	assert.equal(ids(moveShots(film, ['a'], 99)), 'bcdea');
	// Where a drag lands: in front of the first shot whose middle is after the pointer.
	const parts = [
		{ start: 0, length: 2 },
		{ start: 2, length: 4 },
		{ start: 6, length: 2 }
	];
	assert.deepEqual([0.5, 1.5, 3.9, 4.1, 6.9, 7.5, 99].map((t) => dropPlace(parts, t)), [0, 1, 1, 2, 2, 3, 3]);
});

test('trims are whole frames and leave half a second', () => {
	const shot = { trim_start: 0, trim_end: 0 };
	// A 56-frame clip: the left edge goes right by 0.52 s, which is 12 frames.
	assert.deepEqual(trimShot(shot, 56 / 24, 'start', 0.52), { trim_start: 0.5, trim_end: 0 });
	// The right edge goes left.
	assert.deepEqual(trimShot(shot, 56 / 24, 'end', -0.75), { trim_start: 0, trim_end: 0.75 });
	// Dragging outwards cuts nothing, and less than nothing is nothing.
	assert.deepEqual(trimShot(shot, 56 / 24, 'start', -3), { trim_start: 0, trim_end: 0 });
	assert.deepEqual(trimShot({ trim_start: 0.5, trim_end: 0.25 }, 56 / 24, 'end', 9), { trim_start: 0.5, trim_end: 0 });
	// Half a second always stays: 56 frames less 12 is 44 frames to cut at most, shared by both ends.
	assert.deepEqual(trimShot(shot, 56 / 24, 'start', 9), { trim_start: 44 / 24, trim_end: 0 });
	assert.deepEqual(trimShot({ trim_start: 1, trim_end: 0 }, 56 / 24, 'end', -9), { trim_start: 1, trim_end: 20 / 24 });
	// A join handle: near the cut it is a cut; farther it is a dissolve above and a fade below.
	assert.deepEqual(joinFromDrag(0.05, true), { join: 'cut', join_seconds: 0.5 });
	assert.deepEqual(joinFromDrag(-0.74, true), { join: 'dissolve', join_seconds: 0.75 });
	assert.deepEqual(joinFromDrag(0.2, false), { join: 'black', join_seconds: 0.25 });
	assert.deepEqual(joinFromDrag(7, false), { join: 'black', join_seconds: 2 });
});

test('the plan of every shot is the timeline, joins as long as they say', () => {
	const shot = (id, over = {}) => ({ id, prompt: id, frames: 49, state: 'done', made_frames: 48, ...over });
	const film = [shot('a'), shot('b', { join: 'dissolve', join_seconds: 1 }), shot('x', { state: 'empty', frames: 121 }), shot('c', { join: 'black', join_seconds: 2 })];
	const made = filmPlan(film);
	const all = filmPlan(film, true);
	assert.deepEqual(made.parts.map((p) => p.shot), ['a', 'b', 'c']);
	assert.deepEqual(all.parts.map((p) => [p.shot, p.made, p.start, p.length]), [['a', true, 0, 2], ['b', true, 1, 2], ['x', false, 3, 121 / 24], ['c', true, 3 + 121 / 24, 2]]);
	// One second of dissolve; a fade through black of two seconds is one down and one up.
	assert.deepEqual(made.parts.map((p) => [p.join, p.overlap, p.fadeIn, p.fadeOut]), [['cut', 0, 0, 0], ['dissolve', 1, 0, 1], ['black', 0, 1, 0]]);
	assert.equal(made.total, 5);
	// How much of a part is seen: its dip to black, and the film's own fades.
	const b = made.parts[1];
	assert.equal(partLight(b, 1.5, 5), 1);
	assert.equal(partLight(b, 2.5, 5), 0.5);
	assert.equal(partLight(made.parts[2], 3.25, 5), 0.25);
	assert.equal(partLight(made.parts[0], 0.25, 5, 0.5, 0.5), 0.5);
	close(partLight(made.parts[2], 4.9, 5, 0.5, 0.5), 0.2, 1e-9);
});

test('regions move, trim and split', () => {
	const r = region('r', 4, 3, { offset: 1, fade_in: 0.5, fade_out: 1 });
	assert.equal(regionEnd(r), 7);
	assert.equal(moveRegion(r, -2).start, 0);
	// The left edge moves right: the end stays, the region starts later in its file.
	assert.deepEqual(trimRegionStart(r, 5), { ...r, start: 5, offset: 2, length: 2 });
	// It cannot start before its file does (one second earlier at most here).
	assert.deepEqual(trimRegionStart(r, 0), { ...r, start: 3, offset: 0, length: 4 });
	// It cannot be trimmed away, and its fades shrink with it.
	const sliver = trimRegionStart(r, 99);
	close(sliver.length, 2 / 24);
	close(sliver.fade_in + sliver.fade_out, 2 / 24);
	// The right edge: to the end of the file at most.
	assert.equal(trimRegionEnd(r, 9).length, 5);
	assert.equal(trimRegionEnd(r, 9, 5).length, 4);
	close(trimRegionEnd(r, 0).length, 2 / 24);
	// The right edge of a repeated region gives it more or fewer repeats; its left edge stays.
	const looped = region('l', 0, 2, { loops: 3 });
	assert.equal(trimRegionEnd(looped, 9.2).loops, 5);
	assert.equal(trimRegionEnd(looped, 0.3).loops, 1);
	assert.equal(trimRegionStart(looped, 1), looped);

	// A split keeps the first id, and each fade at its own end.
	let n = 0;
	const id = () => `n${++n}`;
	const [a, b] = splitRegion(r, 5.5, id);
	assert.deepEqual([a.id, a.start, a.offset, a.length, a.fade_in, a.fade_out], ['r', 4, 1, 1.5, 0.5, 0]);
	assert.deepEqual([b.start, b.offset, b.length, b.fade_in, b.fade_out], [5.5, 2.5, 1.5, 0, 1]);
	assert.notEqual(b.id, 'r');
	assert.equal(splitRegion(r, 4, id), null);
	assert.equal(splitRegion(r, 7, id), null);
	assert.equal(splitRegion(r, 20, id), null);
	// A repeated region: at a repeat's edge it becomes two repeated regions.
	const two = splitRegion(region('l', 0, 2, { loops: 5 }), 4, id);
	assert.deepEqual(two.map((x) => [x.start, x.length, x.loops]), [[0, 2, 2], [4, 2, 3]]);
	// Inside a repeat: the whole repeats before, the two halves of the cut one, the repeats after.
	const four = splitRegion(region('l', 1, 2, { loops: 4, offset: 0.5 }), 6, id);
	assert.deepEqual(four.map((x) => [x.start, x.offset, x.length, x.loops]), [[1, 0.5, 2, 2], [5, 0.5, 1, 1], [6, 1.5, 1, 1], [7, 0.5, 2, 1]]);
	assert.equal(new Set(four.map((x) => x.id)).size, 4);
	assert.equal(four.reduce((sum, x) => sum + x.length * x.loops, 0), 8);
});

test('regions that lie over each other are known', () => {
	const lane = [region('a', 0, 2), region('b', 2, 2), region('c', 3.5, 1), region('d', 6, 1), region('e', 0.5, 0.5)];
	assert.deepEqual([...overlapping(lane)].sort(), ['a', 'b', 'c', 'e']);
	// Touching ends do not overlap; a long region under several short ones overlaps them all.
	assert.equal(overlapping([region('a', 0, 2), region('b', 2, 2)]).size, 0);
	assert.deepEqual([...overlapping([region('long', 0, 10), region('x', 1, 1), region('y', 5, 1)])].sort(), ['long', 'x', 'y']);
	assert.equal(overlapping([region('l', 0, 1, { loops: 4 }), region('m', 3.5, 1)]).size, 2);
	// A paste sets the copies down from the playhead, as far apart as they were.
	let n = 0;
	const pasted = pasteRegions([region('a', 4, 1), region('b', 6, 2)], 10, () => `p${++n}`);
	assert.deepEqual(pasted.map((x) => [x.id, x.start, x.length]), [['p1', 10, 1], ['p2', 12, 2]]);
	assert.deepEqual(pasteRegions([], 3), []);
	// Fades shape a region at its ends.
	const faded = region('f', 0, 4, { fade_in: 1, fade_out: 2 });
	assert.deepEqual([0, 0.5, 1, 2, 3, 4, 5, -1].map((t) => fadeLevel(faded, t)), [0, 0.5, 1, 1, 0.5, 0, 0, 0]);
});

test('undo keeps what the film was, a hundred steps and more', () => {
	const h = new History();
	let film = 0;
	const edit = (label, key, at) => {
		h.push(film, label, key, at);
		film += 1;
	};
	for (let i = 0; i < 150; i++) edit(`step ${i}`, '', i);
	assert.equal(h.steps, 150);
	assert.equal(h.undoLabel, 'step 149');
	// Undo walks back and redo forward again.
	for (let i = 0; i < 120; i++) film = h.undo(film).state;
	assert.equal(film, 30);
	assert.equal(h.redoLabel, 'step 30');
	film = h.redo(film).state;
	film = h.redo(film).state;
	assert.equal(film, 32);
	// A new edit after an undo ends what could be redone.
	edit('another', '', 1000);
	assert.equal(h.redo(film), null);
	assert.equal(h.undo(film).state, 32);
	// Typing is one step: edits with the same key, a moment apart, belong together.
	const t = new History();
	let text = '';
	const type = (ch, at) => {
		t.push(text, 'Description', 'prompt:a', at);
		text += ch;
	};
	type('f', 0);
	type('o', 300);
	type('x', 900);
	type('!', 5000);
	assert.equal(t.steps, 2);
	assert.equal(t.undo(text).state, 'fox');
	assert.equal(t.undo('fox').state, '');
	assert.equal(t.undo(''), null);
	// The oldest steps go when there are too many.
	const small = new History(3);
	for (let i = 0; i < 10; i++) small.push(i, 'n');
	assert.equal(small.steps, 3);
	assert.equal(small.undo(10).state, 9);
});

test('faders, meters, pan and solo', () => {
	close(dbOf(1), 0);
	close(dbOf(0.5), -6.0206, 1e-4);
	assert.equal(dbOf(0), -Infinity);
	close(gainOf(6), 1.9953, 1e-4);
	assert.equal(gainOf(-Infinity), 0);
	assert.deepEqual([1, 2, 0.5, 0.25, 0, 1.4962].map(dbLabel), ['0.0', '+6.0', '-6.0', '-12.0', '-∞', '+3.5']);
	// The fader: unity at three quarters of its travel, +6 dB at the top, silence at the bottom.
	close(faderToGain(0.76), 1);
	close(faderToGain(1), 1.9953, 1e-4);
	assert.equal(faderToGain(0), 0);
	close(dbOf(faderToGain(0.62)), -6, 1e-9);
	close(dbOf(faderToGain(0.55)), -9, 1e-9);
	close(dbOf(faderToGain(0.04)), -60, 1e-9);
	close(faderToGain(0.02), 0.0005, 1e-9);
	// A strip that goes no louder than as made stops there.
	assert.equal(faderToGain(1, 1.5), 1.5);
	// There and back.
	for (const g of [0.0003, 0.001, 0.05, 0.25, 0.5, 1, 1.5, 1.99]) close(faderToGain(gainToFader(g)), g, 1e-9);
	assert.equal(gainToFader(0), 0);
	assert.equal(gainToFader(9), 1);
	assert.ok(meterFill(1) > meterFill(0.5) && meterFill(0.5) > meterFill(0.01) && meterFill(0) === 0);
	// Pan turns down the side away from it.
	assert.deepEqual(balance(0), [1, 1]);
	assert.deepEqual(balance(-1), [1, 0]);
	assert.deepEqual(balance(0.5), [0.5, 1]);
	assert.deepEqual(balance(7), [0, 1]);
	assert.deepEqual([0, -0.32, 1].map(panLabel), ['C', 'L32', 'R100']);
	// Mute and solo, as the export has them.
	const strips = (list) => list.map(([id, mute, solo]) => ({ id, mute, solo }));
	assert.deepEqual(heard(strips([['shots', false, false], ['a', true, false], ['b', false, false]])), { shots: true, a: false, b: true });
	assert.deepEqual(heard(strips([['shots', false, false], ['a', false, true], ['b', false, false]])), { shots: false, a: true, b: false });
	assert.deepEqual(heard(strips([['shots', false, true], ['a', true, true], ['b', false, false]])), { shots: true, a: false, b: false });
	assert.deepEqual([stripGain(0.8, true), stripGain(0.8, false), stripGain(9, true), stripGain(-1, true)], [0.8, 0, 2, 0]);
});

test('a film goes to the workstation and back to the service', () => {
	const project = {
		id: 'p',
		name: 'Fox',
		folder: '/films/fox',
		size: '640x384',
		updated_ms: 1,
		fade_in: true,
		shots: [
			{ id: 'a1', prompt: 'a fox', frames: 49, state: 'done', made_frames: 56, takes: [{ n: 1, frames: 49 }], take: 1 },
			{ id: 'b2', prompt: 'a boat', frames: 121, state: 'empty', join: 'dissolve', join_seconds: 1.25, continues: true, volume: 0.5, mute: true, seed: 7, trim_start: 0.5 }
		],
		tracks: [{ id: 't1', name: 'Music', color: '#4f8fd9', regions: [{ id: 'r1', file: '/m/theme.wav', start: 2, length: 8, loops: 3 }] }]
	};
	const doc = docOf(project);
	// What the service knows about clips is not part of the film an edit changes.
	assert.deepEqual(Object.keys(doc.shots[0]).sort(), ['continues', 'frames', 'from', 'id', 'join', 'join_seconds', 'lines', 'mute', 'prompt', 'seed', 'trim_end', 'trim_start', 'volume']);
	assert.deepEqual([doc.shots[0].join, doc.shots[0].join_seconds, doc.shots[0].volume, doc.shots[1].join_seconds], ['cut', 0.5, 1, 1.25]);
	assert.deepEqual([doc.master, doc.fade_in, doc.fade_out, doc.shot_track], [1, true, false, { volume: 1, pan: 0, mute: false, solo: false }]);
	assert.deepEqual(doc.tracks[0].regions[0], { id: 'r1', file: '/m/theme.wav', start: 2, offset: 0, length: 8, gain: 1, fade_in: 0, fade_out: 0, loops: 3 });
	const body = saveBody(doc);
	assert.deepEqual(body.shots[1], { id: 'b2', prompt: 'a boat', frames: 121, continues: true, trim_start: 0.5, trim_end: 0, join: 'dissolve', join_seconds: 1.25, volume: 0.5, mute: true, seed: 7, lines: [], from: '' });
	// A film from before casts and lines has none, shows its subtitles and turns the shots down under speech.
	assert.deepEqual([doc.look, doc.cast, doc.seed, doc.speech, doc.dub, doc.subtitles, doc.duck], ['', [], null, false, '', { show: true, burn: false }, 0.6]);
	assert.deepEqual([body.look, body.cast, body.seed, body.voice, body.subtitles, body.duck], ['', [], null, null, { show: true, burn: false }, 0.6]);
	assert.equal(body.tracks[0].regions[0].loops, 3);
	// The body is a copy: changing it leaves the film alone.
	body.tracks[0].regions[0].start = 99;
	assert.equal(doc.tracks[0].regions[0].start, 2);
	// Ids are the kind the service takes.
	const id = newId();
	assert.match(id, /^[0-9a-z]{12}$/);
	assert.notEqual(id, newId());
});

test('a film with a cast and lines goes to the service and back as it is', () => {
	const project = {
		id: 'p',
		name: 'Boat',
		folder: '/f',
		size: '640x384',
		updated_ms: 1,
		look: 'soft watercolour',
		seed: 1234,
		speech: true,
		language: 'English',
		voice: 3,
		duck: 0.4,
		dub: 'Turkish',
		subtitles: { show: false, burn: true },
		cast: [{ id: 'm1', name: 'Mira', look: 'a girl of eight', voice: 7, portrait: 2 }, { id: 'f1', name: 'Fox' }],
		shots: [{ id: 'a1', prompt: 'Mira kneels', frames: 121, state: 'done', from: 'm1', lines: [{ id: 'l1', who: 'm1', text: 'Go on!', said: { Turkish: 'Hadi!' } }, { id: 'l2', text: 'And it went.' }] }]
	};
	const doc = docOf(project);
	assert.deepEqual(doc.cast, [{ id: 'm1', name: 'Mira', look: 'a girl of eight', voice: 7 }, { id: 'f1', name: 'Fox', look: '', voice: null }]);
	assert.deepEqual(doc.shots[0].lines, [{ id: 'l1', who: 'm1', text: 'Go on!', said: { Turkish: 'Hadi!' } }, { id: 'l2', who: '', text: 'And it went.', said: {} }]);
	const body = saveBody(doc);
	assert.deepEqual([body.look, body.seed, body.speech, body.language, body.voice, body.duck, body.dub, body.subtitles], ['soft watercolour', 1234, true, 'English', 3, 0.4, 'Turkish', { show: false, burn: true }]);
	assert.deepEqual([body.shots[0].from, body.shots[0].lines[0].said], ['m1', { Turkish: 'Hadi!' }]);
	// The body is a copy all the way down.
	body.shots[0].lines[0].said.Turkish = 'x';
	body.cast[0].name = 'x';
	assert.deepEqual([doc.shots[0].lines[0].said.Turkish, doc.cast[0].name], ['Hadi!', 'Mira']);
});

test('a shot is told the look of who it names, as the service tells it', () => {
	const cast = [
		{ id: 'm1', name: 'Mira', look: 'a girl of eight in a yellow raincoat and red boots, short dark hair.', voice: null },
		{ id: 'f1', name: 'the Fox', look: 'a small red fox with a white-tipped tail', voice: null },
		{ id: 'x1', name: 'Ada', look: '', voice: null }
	];
	const look = 'soft watercolour, muted winter colours';
	// The same sentences as daemon/src/studio/cast.rs puts together.
	assert.equal(
		told(look, cast, 'Mira kneels by the stream and the fox watches from the trees'),
		'Mira kneels by the stream and the fox watches from the trees. Mira: a girl of eight in a yellow raincoat and red boots, short dark hair. the Fox: a small red fox with a white-tipped tail. soft watercolour, muted winter colours.'
	);
	assert.equal(told(look, cast, 'A wide view of the forest.'), 'A wide view of the forest. soft watercolour, muted winter colours.');
	assert.equal(told('', cast, 'The admiral salutes Ada.'), 'The admiral salutes Ada.');
	assert.equal(told('', [], 'a fox in snow'), 'a fox in snow');
	assert.equal(told(look, cast, '  '), '');
	assert.deepEqual([names("MIRA's boat", 'Mira'), names('admiral', 'Mira'), names('Mira', ''), names('İpek ve Çağrı', 'çağrı')], [true, false, false, true]);
});

test('subtitles: the one on screen, its rows, and the shots under it', () => {
	const cues = [{ start: 0.5, end: 2, text: 'A fox walks.' }, { start: 2, end: 3, text: 'It stops.' }];
	assert.deepEqual([0, 0.5, 1.99, 2, 3, 9].map((t) => cueAt(cues, t)?.text), [undefined, 'A fox walks.', 'A fox walks.', 'It stops.', undefined, undefined]);
	// Rows are broken where the text is too wide for the picture; a row the service broke stays broken.
	const wide = (t) => t.length * 10;
	assert.deepEqual(captionRows('A fox walks through the snow', 120, wide), ['A fox walks', 'through the', 'snow']);
	assert.deepEqual(captionRows('A fox walks\nthrough the snow', 400, wide), ['A fox walks', 'through the snow']);
	assert.deepEqual(captionRows('one two three four five six seven eight', 80, wide, 2), ['one two', 'three four five six seven eight']);
	assert.deepEqual(captionRows('', 100, wide), []);
	// The shots' sound: down under a line, a quarter of a second to get there and back.
	const said = [[2, 4], [4.2, 5]];
	const level = (t) => Math.round(duckLevel(said, 0.6, t) * 1000) / 1000;
	assert.deepEqual([0, 1.75, 1.875, 2, 3, 4.1, 5.125, 5.25, 9].map(level), [1, 1, 0.7, 0.4, 0.4, 0.4, 0.7, 1, 1]);
	assert.equal(duckLevel(said, 0, 3), 1);
	assert.equal(duckLevel([], 0.6, 3), 1);
});

test('work the service did is put into the film as it was edited meanwhile', () => {
	const film = (over = {}) =>
		docOf({ id: 'p', name: 'Boat', folder: '/f', size: '640x384', updated_ms: 1, shots: [{ id: 'a1', prompt: 'a fox', frames: 121, state: 'done', lines: [{ id: 'l1', text: 'A fox walks.' }] }, { id: 'b2', prompt: '', frames: 96, state: 'done' }], ...over });
	const mine = film();
	mine.name = 'Renamed while it worked';
	mine.shots[0].prompt = 'a red fox';
	// A dub: the voice track and the translations come in; what was edited stays.
	const voice = { id: 'v1', name: 'Turkish voice', kind: 'voice', lang: 'Turkish', color: '#4f8fd9', regions: [{ id: 'r1', file: '/f/voice/x.wav', start: 0.15, length: 1.5, line: 'a1/l1' }] };
	const dubbed = film({ dub: 'Turkish', tracks: [voice], shots: [{ id: 'a1', prompt: 'a fox', frames: 121, state: 'done', lines: [{ id: 'l1', text: 'A fox walks.', said: { Turkish: 'Bir tilki yürüyor.' } }] }, { id: 'b2', prompt: '', frames: 96, state: 'done' }] });
	const afterDub = takeWork('dub', mine, dubbed);
	assert.deepEqual([afterDub.name, afterDub.shots[0].prompt, afterDub.dub, afterDub.tracks.map((t) => [t.kind, t.lang, t.regions.length])], ['Renamed while it worked', 'a red fox', 'Turkish', [['voice', 'Turkish', 1]]]);
	assert.deepEqual(afterDub.shots[0].lines[0].said, { Turkish: 'Bir tilki yürüyor.' });
	// A second dub replaces its own track and adds none.
	assert.equal(takeWork('dub', afterDub, dubbed).tracks.length, 1);
	// A video brought in: its shots come in behind the ones here.
	const brought = film({ size: '384x640', export: { scale: 2 }, shots: [{ id: 'a1', prompt: 'a fox', frames: 121, state: 'done' }, { id: 'b2', prompt: '', frames: 96, state: 'done' }, { id: 'c3', prompt: '', frames: 120, state: 'done', take: 1, takes: [{ n: 1, frames: 120, ext: 'mp4' }] }] });
	const afterImport = takeWork('import', mine, brought);
	assert.deepEqual([afterImport.shots.map((s) => s.id), afterImport.size, afterImport.takes.c3, afterImport.shots[0].prompt], [['a1', 'b2', 'c3'], '640x384', 1, 'a red fox']);
	// Only an empty film takes the shape of the video.
	assert.deepEqual([takeWork('import', film({ shots: [] }), brought).size, takeWork('import', film({ shots: [] }), brought).export.scale], ['384x640', 2]);
	// Clips listened to: lines for the shots that had none, and no others.
	const heard = film({ language: 'English', shots: [{ id: 'a1', prompt: 'a fox', frames: 121, state: 'done', lines: [{ id: 'x9', text: 'Heard instead.' }] }, { id: 'b2', prompt: '', frames: 96, state: 'done', lines: [{ id: 'l7', text: 'Then the boat came.' }] }] });
	const afterListen = takeWork('listen', mine, heard);
	assert.deepEqual([afterListen.shots[0].lines[0].text, afterListen.shots[1].lines[0].text, afterListen.language], ['A fox walks.', 'Then the boat came.', 'English']);
	// The film handed in is left as it was.
	assert.deepEqual([mine.tracks.length, mine.shots.length, mine.shots[1].lines.length], [0, 2, 0]);
});
