// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { commandLine, formArgs, needsLine, placeholderIn, quoteArg, skeleton, stateWords, tryArguments } from './connectors.ts';

test('the command is shown as it would be typed', () => {
	assert.equal(quoteArg('-y'), '-y');
	assert.equal(quoteArg('@scope/server'), '@scope/server');
	assert.equal(quoteArg('/Users/me/My Files'), `'/Users/me/My Files'`);
	assert.equal(quoteArg(`it's`), `'it'\\''s'`);
	assert.equal(quoteArg(''), `''`);
	assert.equal(quoteArg('a;rm -rf x'), `'a;rm -rf x'`);
	assert.equal(commandLine(' npx ', ['-y', '@scope/server', '/Users/me/My Files']), `npx -y @scope/server '/Users/me/My Files'`);
	assert.equal(commandLine('', []), '');
});

test('arguments are one on each line and the placeholder is what the person chose', () => {
	assert.deepEqual(formArgs(' -y \n\n@scope/server\n<folder>\n', '/Users/me/notes'), ['-y', '@scope/server', '/Users/me/notes']);
	assert.deepEqual(formArgs('--db-path\n<file>', ''), ['--db-path', '<file>'], 'nothing chosen: the placeholder stays and the service refuses it');
	assert.deepEqual(formArgs('one two', ''), ['one two'], 'a line is one argument, spaces and all');
	assert.equal(placeholderIn('-y\n<folder>'), '<folder>');
	assert.equal(placeholderIn('--db-path\n <file> '), '<file>');
	assert.equal(placeholderIn('--folder=<folder>'), null);
});

test('what a ready-made connector needs is said, and whether it is there', () => {
	assert.deepEqual(needsLine('node', { npx: true, uvx: false }), { text: 'Needs Node.js on this computer. It is installed.', missing: false });
	assert.ok(needsLine('node', { npx: false, uvx: true }).missing);
	assert.ok(needsLine('python', { npx: true, uvx: false }).text.includes('uvx program was not found'));
	assert.ok(!needsLine('python', { npx: false, uvx: true }, 'Mac').missing);
	assert.ok(needsLine('python', { npx: false, uvx: true }, 'Mac').text.startsWith('Needs Python with uv on this Mac.'));
});

test('a try starts from the needed arguments', () => {
	const schema = { properties: { title: { type: 'string' }, count: { type: 'integer' }, tags: { type: ['array', 'null'] }, extra: { type: 'string' } }, required: ['title', 'count', 'tags'] };
	assert.deepEqual(JSON.parse(skeleton(schema)), { title: '', count: 0, tags: [] });
	assert.deepEqual(JSON.parse(skeleton({ properties: { a: {}, b: { type: 'boolean' } } })), { a: '', b: false }, 'none needed: the first few');
	assert.equal(skeleton(null), '{}');
	assert.deepEqual(tryArguments(''), { value: {} });
	assert.deepEqual(tryArguments('{"title":"Milk"}'), { value: { title: 'Milk' } });
	assert.ok('problem' in tryArguments('[1]'));
	assert.ok('problem' in tryArguments('{broken'));
});

test('the state in a few words', () => {
	assert.equal(stateWords({ state: 'running', tools: 1, error: null }), 'Running, 1 tool');
	assert.equal(stateWords({ state: 'running', tools: 14, error: null }), 'Running, 14 tools');
	assert.equal(stateWords({ state: 'stopped', tools: 0, error: null }), 'Stopped');
	assert.equal(stateWords({ state: 'failed', tools: 0, error: 'The program x was not found on this computer.' }), 'Did not start. The program x was not found on this computer.');
	assert.equal(stateWords({ state: 'starting', tools: 0, error: null }), 'Starting');
});
