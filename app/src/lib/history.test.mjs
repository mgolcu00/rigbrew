// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { codeRows, folderName, mayRetitle, tabFrom, tabOf, titleSource } from './history.ts';

const task = (id, folder, state, created_ms, more = {}) => ({ id, folder, title: `task ${id}`, state, created_ms, ...more });

test('the tasks of every folder are one list, newest request first', () => {
	const rows = codeRows([
		task('a', '/Users/me/site', 'done', 100),
		task('b', '/Users/me/api', 'running', 200, { request_ms: 200 }),
		// An old task the person just wrote to again goes to the top.
		task('c', 'C:\\work\\shop', 'asking', 50, { request_ms: 900 }),
		task('d', '/Users/me/site/', 'waiting', 300, { title: '  ' })
	]);
	assert.deepEqual(rows.map((r) => r.id), ['c', 'd', 'b', 'a']);
	assert.deepEqual(rows.map((r) => r.where), ['shop', 'site', 'api', 'site']);
	assert.deepEqual(rows.map((r) => r.mark), ['waiting', 'waiting', 'working', '']);
	assert.equal(rows[1].title, 'Untitled task');
});

test('a task that is working keeps its place while it changes', () => {
	const before = codeRows([task('a', '/p', 'running', 100, { request_ms: 100, updated_ms: 100 }), task('b', '/q', 'done', 200, { updated_ms: 200 })]);
	const after = codeRows([task('a', '/p', 'running', 100, { request_ms: 100, updated_ms: 9999 }), task('b', '/q', 'done', 200, { updated_ms: 200 })]);
	assert.deepEqual(before.map((r) => r.id), after.map((r) => r.id));
});

test('the folder name is the last part of the path', () => {
	assert.equal(folderName('/Users/me/site'), 'site');
	assert.equal(folderName('C:\\work\\shop\\'), 'shop');
	assert.equal(folderName('/'), '/');
});

test('the tab follows the screens that have a history, and a remembered one must still be shown', () => {
	assert.equal(tabOf('code'), 'code');
	assert.equal(tabOf('studio'), 'studio');
	assert.equal(tabOf('models'), null);
	assert.equal(tabFrom('code', ['chat', 'code', 'studio']), 'code');
	assert.equal(tabFrom('studio', ['chat', 'code']), 'chat');
	assert.equal(tabFrom(null, ['chat', 'code']), 'chat');
});

test('a chat is named after the first thing the person wrote and the start of the answer', () => {
	const messages = [
		{ role: 'user', content: ' How do I bake bread? ' },
		{ role: 'assistant', content: '', state: 'failed' },
		{ role: 'assistant', content: 'Mix flour, water, salt and yeast.', state: 'done' },
		{ role: 'user', content: 'And without yeast?' },
		{ role: 'assistant', content: 'Use a sourdough starter.', state: 'done' },
		{ role: 'user', content: 'How long does the starter take?' }
	];
	assert.equal(titleSource([]), null);
	assert.equal(titleSource([{ role: 'image', content: '' }]), null);
	const first = titleSource(messages);
	assert.deepEqual([first.text, first.answer, first.thin], ['How do I bake bread?', 'Mix flour, water, salt and yeast.', true]);
	const again = titleSource(messages, true);
	assert.equal(again.text, 'How do I bake bread?\nAnd without yeast?\nHow long does the starter take?');
	assert.equal(again.answer, 'Use a sourdough starter.');
	assert.equal(again.thin, false);
	// Enough to go by the first time: it is not named again.
	assert.equal(titleSource([{ role: 'user', content: 'x'.repeat(200) }]).thin, false);
});

test('only a title made of the first words, or a thin one a few turns later, is written again', () => {
	assert.equal(mayRetitle('words', false, 1), 'first');
	assert.equal(mayRetitle('model', true, 2), null);
	assert.equal(mayRetitle('model', true, 3), 'again');
	assert.equal(mayRetitle('model', false, 9), null);
	// The person named it, or it is a chat from before titles said who wrote them.
	assert.equal(mayRetitle('person', true, 9), null);
	assert.equal(mayRetitle('', false, 9), null);
	assert.equal(mayRetitle(undefined, undefined, 9), null);
});
