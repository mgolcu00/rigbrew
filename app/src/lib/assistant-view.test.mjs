// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { changesDone, diffRows, filmActivity, filmLine, filmModes, firstPrompts, foldFilm, heaviest, readsLine } from './assistant-view.ts';

const tool = (name, args, status = 'done', result = null, diff = null) => ({ kind: 'tool', call: `${name}${JSON.stringify(args)}`, name, arguments: args, status, result, diff });

test('every change to the film is one quiet line, and readings fold into one', () => {
	const steps = [
		{ kind: 'user', text: 'make it a night scene' },
		tool('read_film', {}),
		tool('read_shot', { shot: 2 }),
		tool('read_shot', { shot: 3 }),
		tool('update_plan', { steps: [] }),
		tool('edit_shot', { shot: 1, description: 'night' }, 'done', 'Shot 1: description changed.\nShot 1 was made before: it is marked changed until it is made again.', '- a street\n+ a street at night'),
		tool('look_at_shot', { shot: 1 }),
		tool('make_shots', {}, 'waiting'),
		{ kind: 'model', text: 'Done.' }
	];
	const rows = foldFilm(steps);
	assert.deepEqual(
		rows.map((r) => r.kind),
		['user', 'reads', 'step', 'reads', 'step', 'model']
	);
	assert.equal(readsLine(rows[1].steps), 'Read the film and read shots 2 and 3');
	assert.equal(readsLine(rows[3].steps), 'Looked at shot 1');
	assert.deepEqual(filmLine(rows[2].step), { text: 'Shot 1: description changed', tail: '' });
	assert.deepEqual(filmLine(rows[4].step), { text: 'Make shots', tail: 'waiting for you' });
	assert.deepEqual(diffRows(rows[2].step.diff), [
		{ sign: '-', text: 'a street' },
		{ sign: '+', text: 'a street at night' }
	]);
});

test('a step that is under way, refused or stopped says what it was for', () => {
	assert.deepEqual(filmLine(tool('cast_add', { name: 'Ayşe', look: 'red coat' }, 'running')), { text: 'Add Ayşe to the cast…', tail: '' });
	assert.deepEqual(filmLine(tool('edit_shot', { shot: '9' }, 'failed', 'There is no shot 9.')), { text: 'Shot 9: edit', tail: 'did not work' });
	assert.deepEqual(filmLine(tool('export', {}, 'denied')), { text: 'Export', tail: 'not allowed' });
	assert.deepEqual(filmLine(tool('dub', { language: 'Turkish' }, 'stopped')), { text: 'Dub in Turkish', tail: 'stopped' });
	assert.deepEqual(filmLine(tool('make_shots', {}, 'done', 'Queued 4 shots.\nThey are made one after another.')), { text: 'Queued 4 shots', tail: '' });
	assert.equal(readsLine([tool('read_film', {}, 'running')]), 'Read the film…');
	// A failed plan is a line; a plan that worked is shown as the checklist instead.
	assert.equal(foldFilm([tool('update_plan', {}, 'failed', 'The plan has no steps.')]).length, 1);
});

test('the status word follows what is happening, and the film is read again after each change', () => {
	assert.equal(filmActivity([tool('read_film', {}, 'running')]), 'reading');
	assert.equal(filmActivity([tool('edit_shot', { shot: 1 }, 'running')]), 'writing');
	assert.equal(filmActivity([tool('make_shots', {}, 'running')]), 'running');
	assert.equal(filmActivity([{ kind: 'user', text: 'hi' }]), 'thinking');
	const steps = [tool('read_film', {}), tool('edit_shot', { shot: 1 }), tool('add_shot', {}, 'failed'), tool('set_look', {}, 'running')];
	assert.equal(changesDone(steps), 2);
});

test('the modes, the first prompts and the model are as Code has them', () => {
	assert.deepEqual(
		filmModes.map((m) => m.id),
		['ask', 'read_only', 'bypass']
	);
	assert.equal(firstPrompts.length, 3);
	assert.ok(firstPrompts.every((p) => p.length <= 40));
	const pkg = (id, mb, installed = ['a']) => ({ id, installed, variants: [{ id: 'a', disk_mb: mb }, { id: 'b', disk_mb: mb * 4 }] });
	assert.equal(heaviest([pkg('small', 3000), pkg('large', 15000), pkg('unused', 1000, [])]).id, 'large');
	assert.equal(heaviest([]), null);
});
