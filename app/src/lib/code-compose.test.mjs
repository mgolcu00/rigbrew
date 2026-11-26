// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { appCommand, commands, contextShown, fitted, matching, thoughtFor, typing, withCommand, withMention } from './code-compose.ts';

test('a slash at the start opens the commands, an @ at the start of a word opens the files', () => {
	assert.deepEqual(typing('/', 1), { kind: 'slash', query: '' });
	assert.deepEqual(typing('/rev', 4), { kind: 'slash', query: 'rev' });
	assert.equal(typing('/review the parser', 18), null, 'past the command the menu is closed');
	assert.equal(typing('see /etc', 8), null, 'a slash later in the text is a path');
	assert.deepEqual(typing('look at @src/pa', 15), { kind: 'at', query: 'src/pa', start: 8 });
	assert.deepEqual(typing('@', 1), { kind: 'at', query: '', start: 0 });
	assert.deepEqual(typing('(@a', 3), { kind: 'at', query: 'a', start: 1 });
	assert.equal(typing('mail me@example.com', 19), null, 'an address is no file');
	assert.equal(typing('look at @src/a.rs now', 21), null);
});

test('the commands are the built-in ones and the skills, found by their start first', () => {
	const all = commands([
		{ name: 'write-tests', description: 'Write tests' },
		{ name: 'review', description: 'A skill named like a command' }
	]);
	assert.deepEqual(
		all.map((c) => c.name),
		['init', 'review', 'test', 'explain', 'clear', 'model', 'mode', 'write-tests']
	);
	assert.deepEqual(
		matching(all, 'te').map((c) => c.name),
		['test', 'write-tests']
	);
	assert.deepEqual(
		matching(all, 'mo').map((c) => c.name),
		['model', 'mode']
	);
	assert.equal(matching(all, 'zzz').length, 0);
	assert.equal(withCommand('/rev', 'review'), '/review ');
	assert.equal(withCommand('/t the parser', 'test'), '/test the parser');
});

test('choosing a file puts @path in place of what was typed', () => {
	assert.deepEqual(withMention('fix @pa please', 4, 7, 'src/parse.rs'), { text: 'fix @src/parse.rs please', caret: 17 });
	assert.deepEqual(withMention('@', 0, 1, 'src/'), { text: '@src/', caret: 5 }, 'a folder keeps the menu going');
	assert.deepEqual(withMention('see @pa', 4, 7, 'b.txt'), { text: 'see @b.txt ', caret: 11 });
});

test('the commands done by the app, and the small labels', () => {
	assert.deepEqual(appCommand('/clear'), { name: 'clear', rest: '' });
	assert.deepEqual(appCommand(' /model Big One '), { name: 'model', rest: 'Big One' });
	assert.equal(appCommand('/review'), null);
	assert.equal(appCommand('please /clear'), null);
	assert.equal(contextShown({ used: 2800, size: 8192 }), '34% of context');
	assert.equal(contextShown({ used: 10, size: 32768 }), '1% of context');
	assert.equal(contextShown({ used: 9000, size: 8192 }), '100% of context');
	assert.equal(contextShown(null), '');
	assert.equal(thoughtFor(12_400), 'Thought for 12 s');
	assert.equal(thoughtFor(300), 'Thought for a moment');
	assert.equal(thoughtFor(75_000), 'Thought for 1 min 15 s');
	assert.deepEqual(fitted(3200, 1800), { width: 1600, height: 900 });
	assert.deepEqual(fitted(800, 600), { width: 800, height: 600 });
});
