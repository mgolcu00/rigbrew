// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { MAX_INSTRUCTION, MAX_NAME, MAX_STARTER, agentPrompt, allowedTools, checkAgent, copyName, examples, isValid, newAgentId, starterDraft, tidy, validId } from './agents.ts';

const agent = (over = {}) => ({ id: 'tutor-ab12c', name: 'Tutor', instruction: 'Explain step by step.', tools: [], starter: '', ...over });

test('ids', () => {
	for (const id of ['a', 'editor', 'art-director', 'a1-b2', 'a'.repeat(40)]) assert.ok(validId(id), id);
	for (const id of ['', 'Editor', 'a_b', 'a b', '../x', 'é', 'a'.repeat(41)]) assert.ok(!validId(id), id);
});

test('a good agent has no problems', () => {
	assert.deepEqual(checkAgent(agent()), {});
	assert.ok(isValid(checkAgent(agent({ tools: ['generate_image', 'read_aloud', 'generate_video'], starter: 'Teach me how' }))));
});

test('what is wrong is said field by field', () => {
	assert.ok(checkAgent(agent({ id: 'Bad Id' })).id);
	assert.ok(checkAgent(agent({ name: '   ' })).name);
	assert.ok(checkAgent(agent({ instruction: '\n ' })).instruction);
	assert.ok(checkAgent(agent({ tools: ['run_shell'] })).tools);
	assert.ok(checkAgent(agent({ starter: 'x'.repeat(MAX_STARTER + 1) })).starter);
	const both = checkAgent(agent({ name: '', instruction: '' }));
	assert.deepEqual(Object.keys(both).sort(), ['instruction', 'name']);
});

test('lengths are counted in characters, after trimming', () => {
	assert.ok(isValid(checkAgent(agent({ name: 'ü'.repeat(MAX_NAME) }))));
	assert.ok(isValid(checkAgent(agent({ name: `  ${'n'.repeat(MAX_NAME)}  ` }))));
	assert.ok(checkAgent(agent({ name: 'n'.repeat(MAX_NAME + 1) })).name);
	assert.ok(isValid(checkAgent(agent({ name: '😀'.repeat(MAX_NAME) }))));
	assert.ok(isValid(checkAgent(agent({ instruction: 'ş'.repeat(MAX_INSTRUCTION) }))));
	assert.ok(checkAgent(agent({ instruction: 'i'.repeat(MAX_INSTRUCTION + 1) })).instruction);
});

test('the examples are good agents', () => {
	assert.deepEqual(
		examples.map((e) => e.name),
		['Editor', 'Tutor', 'Art director']
	);
	for (const e of examples) assert.deepEqual(checkAgent(e), {}, e.name);
	assert.deepEqual(examples[2].tools, ['generate_image']);
});

test('tidy trims and orders the abilities', () => {
	const t = tidy(agent({ name: '  Tutor ', instruction: ' Do it. \n', tools: ['generate_video', 'generate_image', 'generate_image'], starter: ' Hello ' }));
	assert.deepEqual(t, { id: 'tutor-ab12c', name: 'Tutor', instruction: 'Do it.', tools: ['generate_image', 'generate_video'], starter: 'Hello', connectors: false, skills: [] });
});

test('connectors are off and no skill is pinned unless asked', () => {
	assert.equal(tidy(agent()).connectors, false);
	assert.equal(tidy(agent({ connectors: true })).connectors, true);
	assert.deepEqual(tidy(agent({ skills: ['write-an-email', 'plan-a-project', 'write-an-email'] })).skills, ['write-an-email', 'plan-a-project']);
	assert.equal(tidy(agent({ skills: Array.from({ length: 20 }, (_, i) => `skill-${i}`) })).skills.length, 12);
});

test('new ids are valid, follow the name and are never taken', () => {
	let n = 0;
	const random = () => (n++ % 36) / 36;
	const id = newAgentId('Çay Ustası!', [], random);
	assert.ok(validId(id), id);
	assert.ok(id.startsWith('cay-ustasi-'), id);
	assert.ok(validId(newAgentId('日本語')));
	assert.ok(newAgentId('日本語').startsWith('agent-'));
	assert.ok(validId(newAgentId('x'.repeat(200))));
	// The first draw is taken: another one is made.
	let calls = 0;
	const fixed = () => (calls++ < 5 ? 0 : 0.5);
	assert.equal(newAgentId('a', ['a-aaaaa'], fixed), 'a-sssss');
});

test('copies get a free name within the limit', () => {
	assert.equal(copyName('Editor', []), 'Editor copy');
	assert.equal(copyName('Editor', ['Editor copy']), 'Editor copy 2');
	assert.equal(copyName('Editor', ['Editor copy', 'Editor copy 2']), 'Editor copy 3');
	assert.ok([...copyName('n'.repeat(MAX_NAME), ['x'])].length <= MAX_NAME);
});

test('an agent is offered only the tools it may use, of those that can be used now', () => {
	const now = ['generate_image', 'read_aloud'];
	assert.deepEqual(allowedTools(null, now), now);
	assert.deepEqual(allowedTools(agent(), now), []);
	assert.deepEqual(allowedTools(agent({ tools: ['read_aloud', 'generate_video'] }), now), ['read_aloud']);
});

test('the prompt carries the name and the instruction', () => {
	const said = agentPrompt(agent({ name: ' Tutor ', instruction: ' Explain. ' }));
	assert.ok(said.includes('"Tutor"'));
	assert.ok(said.endsWith('Explain.'));
});

test('a first message that is not a sentence gets a space to type on from', () => {
	assert.equal(starterDraft('Teach me how'), 'Teach me how ');
	assert.equal(starterDraft('Tighten this text:'), 'Tighten this text: ');
	assert.equal(starterDraft('What should I learn today?'), 'What should I learn today?');
});
