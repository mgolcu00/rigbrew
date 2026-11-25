// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { remadeByMove, reorder, shotSeconds, stamp } from './studio.ts';

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
