// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { groups, siteOf } from './catalog.ts';

const model = (id, kind) => ({ id, kind, name: id, by: '', url: 'https://huggingface.co/a/b', license: '', about: '', status: 'listed' });

test('models are grouped by kind, in a fixed order, under plain names', () => {
	const got = groups([
		model('v', 'video'),
		model('t', 'tool'),
		model('c1', 'chat'),
		model('p', 'image'),
		model('c2', 'chat'),
		model('l', 'listen'),
		model('s', 'speech'),
		model('m', 'music')
	]);
	assert.deepEqual(
		got.map((g) => g.name),
		['Chat', 'Pictures', 'Video', 'Speech', 'Listening', 'Music', 'Tools']
	);
	assert.deepEqual(
		got[0].models.map((m) => m.id),
		['c1', 'c2'],
		'the order of the file is kept inside a group'
	);
});

test('a kind this build has not heard of goes under Other, and empty groups are left out', () => {
	const got = groups([model('x', 'smell'), model('y', ''), model('c', 'chat')]);
	assert.deepEqual(
		got.map((g) => [g.name, g.models.length]),
		[
			['Chat', 1],
			['Other', 2]
		]
	);
	assert.deepEqual(groups([]), []);
});

test('the site is named from the link', () => {
	assert.equal(siteOf('https://github.com/a/b'), 'GitHub');
	assert.equal(siteOf('https://huggingface.co/a/b'), 'Hugging Face');
});
