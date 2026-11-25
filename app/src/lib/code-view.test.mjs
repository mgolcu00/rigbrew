// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { activityOf, diffCount, fold, lineOf, summary } from './code-view.ts';

const tool = (name, args = {}, status = 'done', more = {}) => ({ kind: 'tool', call: `${name}-${Math.random()}`, name, arguments: args, status, result: 'ok', diff: null, ...more });
const said = (kind, text) => ({ kind, text });

test('a run of look-ups is one row, and a change, a command, a question or an answer ends it', () => {
	const steps = [
		said('user', 'fix the test'),
		tool('list_files', { path: '.' }),
		tool('read_file', { path: 'a.py' }),
		tool('search', { pattern: 'def add' }),
		tool('update_plan', { steps: [] }),
		tool('read_file', { path: 'b.py' }),
		tool('edit_file', { path: 'a.py' }, 'done', { diff: '--- a/a.py\n+++ b/a.py\n@@ -1 +1,2 @@\n-x\n+y\n+z' }),
		tool('read_file', { path: 'a.py' }),
		tool('run_command', { command: 'pytest' }),
		tool('web_search', { query: 'pytest fixtures' }),
		tool('fetch_url', { url: 'https://example.com/a' }),
		said('model', 'It passes now.'),
		tool('read_file', { path: 'c.py' }),
		said('question', 'How should I go on?')
	];
	const rows = fold(steps);
	assert.deepEqual(
		rows.map((r) => (r.kind === 'lookups' ? `lookups:${r.steps.length}` : r.kind === 'step' ? r.step.name : r.kind)),
		['user', 'lookups:4', 'edit_file', 'lookups:1', 'run_command', 'lookups:2', 'model', 'lookups:1', 'question']
	);
	// The plan has its own place: updating it is no row and does not end the run.
	assert.equal(rows[1].steps.at(-1).arguments.path, 'b.py');
	assert.equal(rows[1].at, 1);
});

test('a failed plan update is shown, and tools of a connector are never folded', () => {
	const rows = fold([tool('read_file', { path: 'a' }), tool('update_plan', {}, 'failed'), tool('find_tool', { query: 'issues' }), tool('call_tool', { connector: 'git', tool: 'log' }), tool('read_file', { path: 'b' })]);
	assert.deepEqual(
		rows.map((r) => r.kind),
		['lookups', 'step', 'step', 'step', 'lookups']
	);
});

test('the line of a run says what kind of thing was done and how often, never what was searched for', () => {
	const done = summary([
		tool('read_file', { path: 'a.py' }),
		tool('read_file', { path: 'a.py', from_line: 200 }),
		tool('outline', { path: 'b.py' }),
		tool('search', { pattern: 'secret words' }),
		tool('search', { pattern: 'other words' }, 'failed'),
		tool('web_search', { query: 'how to brew' }),
		tool('fetch_url', { url: 'https://example.com' })
	]);
	assert.equal(done.running, false);
	assert.deepEqual(done.parts, [
		{ label: 'Read', count: '2 files' },
		{ label: 'Searched', count: '2 times' },
		{ label: 'Searched the web', count: '' },
		{ label: 'Opened', count: '1 page' }
	]);
	assert.equal(done.failed, 1);
	assert.ok(!JSON.stringify(done).includes('secret') && !JSON.stringify(done).includes('brew'));
	assert.deepEqual(summary([tool('list_files', { path: '.' })]).parts, [{ label: 'Listed files', count: '' }]);
	// While one of them is going, the line says what is going on.
	assert.equal(summary([tool('read_file', { path: 'a' }), tool('search', { pattern: 'x' }, 'running')]).label, 'Searching…');
	assert.equal(summary([tool('web_search', { query: 'x' }, 'running')]).label, 'Searching the web…');
	assert.equal(summary([tool('fetch_url', { url: 'x' }, 'running')]).label, 'Opening a page…');
	assert.equal(summary([tool('read_file', { path: 'x' }, 'running')]).label, 'Reading…');
});

test('a step is a short label and the thing it was about', () => {
	assert.deepEqual(lineOf(tool('run_command', { command: 'cargo test' })), { verb: 'Run', code: 'cargo test' });
	assert.deepEqual(lineOf(tool('edit_file', { path: 'src/a.rs' }, 'done', { diff: '--- a\n+++ b\n@@\n-x\n+y\n+z' })), { verb: 'Edit', code: 'src/a.rs', plus: 2, minus: 1 });
	assert.deepEqual(lineOf(tool('write_file', { path: 'new.txt' }, 'waiting')), { verb: 'Write', code: 'new.txt' });
	assert.deepEqual(lineOf(tool('read_file', { path: 'a.rs', from_line: 40 })), { verb: 'Read', code: 'a.rs:40' });
	assert.deepEqual(lineOf(tool('search', { pattern: 'fn main', path: 'src' })), { verb: 'Search', code: 'fn main  in src' });
	assert.deepEqual(lineOf(tool('fetch_url', { url: 'https://example.com/docs' })), { verb: 'Open', code: 'example.com/docs' });
	assert.deepEqual(lineOf(tool('call_tool', { connector: 'git', tool: 'log' })), { verb: 'Use', code: 'git log' });
	assert.deepEqual(lineOf(tool('something_new')), { verb: 'something_new', code: '' });
	assert.deepEqual(diffCount('--- /dev/null\n+++ b/x\n@@ -0,0 +1,2 @@\n+a\n+b'), { plus: 2, minus: 0 });
});

test('what is happening is read from the step that is going', () => {
	assert.equal(activityOf([]), 'thinking');
	assert.equal(activityOf([said('user', 'go')]), 'thinking');
	assert.equal(activityOf([tool('read_file', {}, 'done')]), 'thinking');
	assert.equal(activityOf([tool('read_file', {}, 'running')]), 'reading');
	assert.equal(activityOf([tool('search', {}, 'running')]), 'searching');
	assert.equal(activityOf([tool('write_file', {}, 'running')]), 'writing');
	assert.equal(activityOf([tool('run_command', {}, 'running')]), 'running');
	assert.equal(activityOf([tool('web_search', {}, 'running')]), 'web');
	assert.equal(activityOf([tool('fetch_url', {}, 'running')]), 'web');
	assert.equal(activityOf([tool('call_tool', {}, 'running')]), 'thinking');
});
