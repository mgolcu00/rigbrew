// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { filmPlan, partAt, remadeByMove, reorder, shotSeconds, stamp } from './studio.ts';

const shot = (id, over = {}) => ({ id, prompt: id, frames: 49, state: 'done', ...over });

test('a shot is as long as its clip', () => {
	assert.equal(shotSeconds(shot('a', { made_frames: 56 })), 56 / 24);
	assert.equal(shotSeconds(shot('a', { made_frames: 56, state: 'empty' })), 49 / 24);
	assert.equal(shotSeconds(shot('a')), 49 / 24);
	assert.equal(stamp(7.4), '0:07');
	assert.equal(stamp(150), '2:30');
});

test('reorder', () => {
	assert.deepEqual(reorder(['a', 'b', 'c', 'd'], 0, 2), ['b', 'c', 'a', 'd']);
	assert.deepEqual(reorder(['a', 'b', 'c', 'd'], 3, 0), ['d', 'a', 'b', 'c']);
	assert.deepEqual(reorder(['a', 'b'], 5, 0), ['a', 'b']);
});

test('a move sends continuing shots back to be made again', () => {
	const film = [shot('a'), shot('b', { continues: true }), shot('c'), shot('d', { continues: true }), shot('e', { continues: true, state: 'empty' })];
	// A shot nothing continues moves in front: every shot still follows the same one.
	assert.deepEqual(remadeByMove(film.slice(0, 3), 2, 0), []);
	// The first shot of a scene moves away: the shot that continued it now follows another.
	assert.deepEqual(remadeByMove(film, 2, 0), [3]);
	// A shot lands between a shot and the one that continues it.
	assert.deepEqual(remadeByMove(film, 2, 1), [2, 3]);
	// A shot that is not made yet is not counted.
	assert.deepEqual(remadeByMove([film[0], film[4]], 1, 0), []);
	// A continuing shot becomes the first.
	assert.deepEqual(remadeByMove(film, 1, 0), [0]);
	assert.deepEqual(remadeByMove(film, 4, 4), []);
});

test('the film plan follows trims and joins', () => {
	const film = [
		shot('a', { made_frames: 48 }),
		shot('b', { made_frames: 48, trim_start: 0.5, trim_end: 0.25, join: 'dissolve' }),
		shot('x', { state: 'empty' }),
		shot('c', { made_frames: 48, join: 'black', volume: 1.2 }),
		shot('d', { made_frames: 48, continues: true, join: 'dissolve', mute: true })
	];
	const { parts, total } = filmPlan(film);
	assert.deepEqual(parts.map((p) => p.shot), ['a', 'b', 'c', 'd']);
	assert.deepEqual(parts.map((p) => [p.from, p.to]), [[0, 2], [0.5, 1.75], [0, 2], [0, 2]]);
	// The dissolve starts half a second before the first shot ends; black and continue do not overlap.
	assert.deepEqual(parts.map((p) => p.start), [0, 1.5, 2.75, 4.75]);
	assert.deepEqual(parts.map((p) => p.overlap), [0, 0.5, 0, 0]);
	assert.deepEqual(parts.map((p) => [p.fadeIn, p.fadeOut]), [[0, 0], [0, 0.25], [0.25, 0], [0, 0]]);
	assert.deepEqual(parts.map((p) => p.volume), [1, 1, 1.2, 0]);
	assert.equal(total, 6.75);
	assert.deepEqual(partAt(parts, 0.4), { part: 0, at: 0.4 });
	assert.deepEqual(partAt(parts, 1.75), { part: 1, at: 0.75 });
	assert.deepEqual(partAt(parts, 99), { part: 3, at: 2 });
	assert.equal(partAt([], 1), null);
	// A join is never longer than half of the shorter shot.
	const short = filmPlan([shot('a', { made_frames: 48 }), shot('b', { made_frames: 12, join: 'dissolve' })]);
	assert.equal(short.parts[1].overlap, 0.25);
	assert.equal(short.total, 2.25);
});
