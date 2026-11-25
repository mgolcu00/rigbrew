// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { NO_REPEAT, elapsed, nextWord, words } from './code-words.ts';

test('the words: many, short, each one once, each ending in an ellipsis', () => {
	const all = Object.values(words).flat();
	assert.ok(all.length >= 120, `${all.length} words`);
	assert.equal(new Set(all.map((w) => w.toLowerCase())).size, all.length, 'no word twice');
	for (const [pool, list] of Object.entries(words)) {
		assert.ok(list.length >= 20, `${pool} has ${list.length}`);
		for (const w of list) {
			assert.ok(w.endsWith('…'), w);
			assert.ok(/^[A-Z]/.test(w), w);
			assert.ok(w.split(' ').length <= 4, w);
			assert.ok(w.length <= 28, w);
		}
	}
	assert.ok(all.filter((w) => w.split(' ').length <= 3).length >= 120, 'at least 120 of one to three words');
});

test('the next word comes from what is happening and is never one of the last few', () => {
	// A source of numbers that walks through the pool, so every word gets its turn.
	let n = 0;
	const walk = () => ((n += 0.37) % 1 < 0.2 ? 0.5 : n % 1);
	const recent = [];
	for (let i = 0; i < 400; i++) {
		const activity = ['running', 'web', 'reading', 'thinking'][i % 4];
		const word = nextWord(activity, recent, walk);
		assert.ok(!recent.slice(-NO_REPEAT).includes(word), `${word} came back too soon`);
		assert.ok(words[activity].includes(word) || words.thinking.includes(word), `${word} for ${activity}`);
		recent.push(word);
	}
	assert.ok(new Set(recent).size > 40);
	// Mostly its own pool; an unknown kind of work falls back to the general one.
	assert.ok(words.running.includes(nextWord('running', [], () => 0.99)));
	assert.ok(words.thinking.includes(nextWord('running', [], () => 0.1)));
	assert.ok(words.thinking.includes(nextWord('nothing known', [], () => 0.5)));
	assert.equal(nextWord('web', [], () => 0.999999), words.web.at(-1));
});

test('the time so far', () => {
	assert.equal(elapsed(0), '0s');
	assert.equal(elapsed(-50), '0s');
	assert.equal(elapsed(59_999), '59s');
	assert.equal(elapsed(125_000), '2m 05s');
	assert.equal(elapsed(3_600_000 + 3 * 60_000 + 9000), '1h 03m');
});
