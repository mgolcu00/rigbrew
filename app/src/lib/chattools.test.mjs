// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import {
	MAX_CHAT_SKILLS,
	NO_EXTRAS,
	callArguments,
	chatConnectors,
	chatSkills,
	extraTools,
	extrasPrompt,
	findConnector,
	hasExtras,
	isExtraTool,
	olderResult,
	shownArguments
} from './chattools.ts';

const skill = (name, over = {}) => ({ name, description: `Does ${name}. Use when asked for ${name}.`, enabled: true, where: 'any', ...over });
const connector = (id, over = {}) => ({ id, name: id, description: '', about: '', state: 'running', tools: 2, always_allow: [], ...over });

test('a chat is offered the skills that are on and not only for Code', () => {
	const all = [skill('a'), skill('b', { enabled: false }), skill('c', { where: 'code' }), skill('d', { where: 'chat' })];
	assert.deepEqual(chatSkills(all).map((s) => s.name), ['a', 'd']);
});

test('an agent with pinned skills is offered those and no others', () => {
	const all = [skill('a'), skill('b'), skill('c', { enabled: false }), skill('d', { where: 'code' })];
	assert.deepEqual(chatSkills(all, ['b', 'c', 'd', 'gone']).map((s) => s.name), ['b']);
	assert.deepEqual(chatSkills(all, []).map((s) => s.name), ['a', 'b']);
});

test('over the limit the shortest descriptions stay, in their own order', () => {
	const many = Array.from({ length: 15 }, (_, i) => skill(`s${String(i).padStart(2, '0')}`, { description: 'x'.repeat(20 + (14 - i) * 5) }));
	const kept = chatSkills(many);
	assert.equal(kept.length, MAX_CHAT_SKILLS);
	assert.deepEqual(kept.map((s) => s.name), many.slice(3).map((s) => s.name));
	// A budget in characters cuts too.
	const long = [skill('long', { description: 'y'.repeat(190) }), skill('short', { description: 'Short one.' }), skill('mid', { description: 'z'.repeat(80) })];
	assert.deepEqual(chatSkills(long, [], 12, 120).map((s) => s.name), ['short', 'mid']);
	assert.deepEqual(chatSkills(long, [], 12, 5), []);
});

test('only running connectors with tools are offered', () => {
	const all = [connector('notes'), connector('off', { state: 'stopped' }), connector('broken', { state: 'failed' }), connector('empty', { tools: 0 })];
	assert.deepEqual(chatConnectors(all).map((c) => c.id), ['notes']);
});

test('the model is told one line for each skill and connector, and never a list of tools', () => {
	assert.equal(extrasPrompt(NO_EXTRAS), '');
	assert.ok(!hasExtras(NO_EXTRAS));
	const extras = {
		skills: [skill('write-an-email', { description: `Write an email.\n${'Very long. '.repeat(40)}` })],
		connectors: [connector('notes', { name: 'My Notes', description: 'Shopping and to-do notes' }), connector('clock', { about: 'Tells the time' }), connector('bare')]
	};
	assert.ok(hasExtras(extras));
	const prompt = extrasPrompt(extras);
	const lines = prompt.split('\n');
	const emailLine = lines.find((l) => l.startsWith('- write-an-email: '));
	assert.ok(emailLine.length <= 220 && emailLine.endsWith('…'), 'a long description is cut to one line');
	assert.ok(lines.includes('- notes (My Notes): Shopping and to-do notes'), 'the words of the person come first');
	assert.ok(lines.includes('- clock: Tells the time'));
	assert.ok(lines.includes('- bare: 2 tools'));
	assert.ok(prompt.includes('use_skill') && prompt.includes('find_tool') && prompt.includes('call_tool'));
	assert.ok(prompt.includes('The user is asked before a tool runs.'));
	assert.ok(!prompt.includes('—'));
	assert.ok(!extrasPrompt({ skills: extras.skills, connectors: [] }).includes('find_tool'));
});

test('the tools follow what is offered', () => {
	const names = (extras) => extraTools(extras).map((t) => t.function.name);
	assert.deepEqual(names(NO_EXTRAS), []);
	assert.deepEqual(names({ skills: [skill('a')], connectors: [] }), ['use_skill', 'read_skill_file']);
	assert.deepEqual(names({ skills: [], connectors: [connector('notes')] }), ['find_tool', 'call_tool']);
	assert.deepEqual(names({ skills: [skill('a')], connectors: [connector('notes')] }), ['use_skill', 'read_skill_file', 'find_tool', 'call_tool']);
	assert.ok(isExtraTool('call_tool') && isExtraTool('use_skill') && !isExtraTool('generate_image') && !isExtraTool('run_command'));
});

test('arguments that are not an object are nothing', () => {
	assert.deepEqual(callArguments('{"name":"a"}'), { name: 'a' });
	assert.deepEqual(callArguments(''), {});
	assert.deepEqual(callArguments('{broken'), {});
	assert.deepEqual(callArguments('[1,2]'), {});
	assert.deepEqual(callArguments('"text"'), {});
	assert.deepEqual(callArguments('null'), {});
});

test('a connector is found by its id or its name', () => {
	const all = [connector('notes', { name: 'My Notes' }), connector('clock')];
	assert.equal(findConnector(all, 'notes')?.id, 'notes');
	assert.equal(findConnector(all, ' My notes ')?.id, 'notes');
	assert.equal(findConnector(all, 'Clock')?.id, 'clock');
	assert.equal(findConnector(all, 'weather'), null);
	assert.equal(findConnector(all, 42), null);
});

test('arguments are shown as they will be sent', () => {
	assert.equal(shownArguments({ title: 'Milk' }), '{\n  "title": "Milk"\n}');
	assert.equal(shownArguments('{"title":"Milk"}'), '{\n  "title": "Milk"\n}');
	assert.equal(shownArguments(undefined), '{}');
	assert.equal(shownArguments('not json'), 'not json');
	assert.ok(shownArguments({ text: 'x'.repeat(5000) }).endsWith('\n…'));
});

test('older results are made small', () => {
	const body = `Skill write-an-email:\n${'step '.repeat(500)}`;
	assert.equal(olderResult('use_skill', '{"name":"write-an-email"}', body), 'The skill write-an-email was read here earlier. Call use_skill again if you need it.');
	assert.equal(olderResult('use_skill', '{"name":"nope"}', 'There is no skill named nope.'), 'There is no skill named nope.');
	assert.equal(olderResult('call_tool', '{}', 'short'), 'short');
	const cut = olderResult('call_tool', '{}', 'r'.repeat(2000));
	assert.ok(cut.length < 460 && cut.endsWith('[the rest was left out to save space]'));
});
