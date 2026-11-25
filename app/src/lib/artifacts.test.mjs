// @ts-nocheck: plain JavaScript on purpose, so it runs without a build step.
// Run with: node --test src/lib/*.test.mjs  (Node 22.18 or newer reads the .ts files as they are)
import assert from 'node:assert/strict';
import { test } from 'node:test';
import { CODE_MIN_LINES, DOCUMENT_MIN_LINES, PAGE_CSP, asksForPiece, collectArtifacts, extensionOf, fileNameOf, kindOf, pageDocument, splitMessage, svgDataUrl, textOf, titleOf } from './artifacts.ts';

const lines = (n, f = (i) => `line ${i}`) => Array.from({ length: n }, (_, i) => f(i + 1)).join('\n');
const fence = (lang, code) => `\`\`\`${lang}\n${code}\n\`\`\``;
const artifacts = (parts) => parts.filter((p) => p.type === 'artifact').map((p) => p.block);

test('an answer without code is one piece of text', () => {
	assert.deepEqual(splitMessage('Hello.\n\nTwo lines.'), [{ type: 'text', text: 'Hello.\n\nTwo lines.' }]);
	assert.deepEqual(splitMessage(''), []);
});

test('a page is an artifact with the text around it kept', () => {
	const text = `Here is the page.\n\n${fence('html', '<!doctype html>\n<html><head><title>Tea &amp; Timer</title></head><body><h1>Other</h1></body></html>')}\n\nOpen it to try.`;
	const parts = splitMessage(text);
	assert.deepEqual(
		parts.map((p) => p.type),
		['text', 'artifact', 'text']
	);
	assert.equal(parts[0].text, 'Here is the page.\n');
	assert.equal(parts[2].text.trim(), 'Open it to try.');
	const [page] = artifacts(parts);
	assert.equal(page.kind, 'page');
	assert.equal(page.lang, 'html');
	assert.equal(page.closed, true);
	assert.equal(page.title, 'Tea & Timer');
	assert.equal(page.lines, 2);
	assert.ok(!page.code.includes('```'));
});

test('a short page fragment is still a page', () => {
	const [page] = artifacts(splitMessage(fence('html', '<h1>Hello <em>there</em></h1>')));
	assert.equal(page.kind, 'page');
	assert.equal(page.title, 'Hello there');
});

test('kinds', () => {
	assert.equal(kindOf('html', '<p>x</p>'), 'page');
	assert.equal(kindOf('HTML', '<p>x</p>'), 'page');
	assert.equal(kindOf('svg', '<svg></svg>'), 'drawing');
	assert.equal(kindOf('html', '  <svg viewBox="0 0 1 1"></svg>'), 'drawing');
	assert.equal(kindOf('xml', '<?xml version="1.0"?>\n<svg xmlns="http://www.w3.org/2000/svg"/>'), 'drawing');
	assert.equal(kindOf('xml', '<note/>'), null);
	assert.equal(kindOf('mermaid', 'graph TD; A-->B'), 'diagram');
	assert.equal(kindOf('markdown', lines(DOCUMENT_MIN_LINES - 1)), null);
	assert.equal(kindOf('md', lines(DOCUMENT_MIN_LINES)), 'document');
	assert.equal(kindOf('python', lines(CODE_MIN_LINES - 1)), null);
	assert.equal(kindOf('python', lines(CODE_MIN_LINES)), 'code');
	assert.equal(kindOf('', lines(CODE_MIN_LINES)), 'code');
	assert.equal(kindOf('', 'ls -la'), null);
});

test('short code stays in the text, exactly as written', () => {
	const text = `Run this:\n\n${fence('sh', 'ls -la')}\n\nDone.`;
	assert.deepEqual(splitMessage(text), [{ type: 'text', text }]);
});

test('long code is a code artifact with its language', () => {
	const [code] = artifacts(splitMessage(`Here.\n${fence('python', lines(40, (i) => `print(${i})`))}`));
	assert.equal(code.kind, 'code');
	assert.equal(code.lang, 'python');
	assert.equal(code.title, 'Code');
	assert.equal(code.lines, 40);
});

test('titles', () => {
	assert.equal(titleOf('page', '<html><body><h1 class="a">My <b>page</b></h1></body></html>'), 'My page');
	assert.equal(titleOf('page', '<div>nothing</div>'), 'Page');
	assert.equal(titleOf('page', '<title>  </title><h1>Fallback</h1>'), 'Fallback');
	assert.equal(titleOf('drawing', '<svg><title>A fox</title></svg>'), 'A fox');
	assert.equal(titleOf('drawing', '<svg></svg>'), 'Drawing');
	assert.equal(titleOf('document', 'intro\n\n## The **plan** for `today` ##\n\ntext'), 'The plan for today');
	assert.equal(titleOf('document', 'no heading here'), 'Document');
	assert.equal(titleOf('diagram', 'graph TD'), 'Diagram');
	assert.equal(titleOf('code', '# not a title'), 'Code');
	assert.equal([...titleOf('page', `<title>${'x'.repeat(200)}</title>`)].length, 80);
	assert.equal(titleOf('page', '<title>&#72;i &#x263A; &lt;there&gt;</title>'), 'Hi ☺ <there>');
});

test('while the answer is being written, an open block is already an artifact', () => {
	const parts = splitMessage('Here is the page.\n\n```html\n<!doctype html>\n<title>Clock</title>\n<bo', false);
	const [page] = artifacts(parts);
	assert.equal(page.kind, 'page');
	assert.equal(page.closed, false);
	assert.equal(page.title, 'Clock');
	assert.equal(page.code, '<!doctype html>\n<title>Clock</title>\n<bo');
});

test('a block that has only just opened is an artifact with nothing in it yet', () => {
	const [page] = artifacts(splitMessage('Sure.\n\n```html', false));
	assert.equal(page.kind, 'page');
	assert.equal(page.closed, false);
	assert.equal(page.code, '');
	assert.equal(page.lines, 0);
	assert.equal(page.title, 'Page');
});

test('half of the closing fence is not shown as code while writing', () => {
	const [page] = artifacts(splitMessage('```html\n<p>x</p>\n``', false));
	assert.equal(page.code, '<p>x</p>');
	assert.equal(page.closed, false);
	const [closed] = artifacts(splitMessage('```html\n<p>x</p>\n```', false));
	assert.equal(closed.closed, true);
});

test('code being written becomes an artifact once it is long enough', () => {
	const short = splitMessage(`\`\`\`js\n${lines(5)}`, false);
	assert.deepEqual(
		short.map((p) => p.type),
		['text']
	);
	const long = artifacts(splitMessage(`\`\`\`js\n${lines(CODE_MIN_LINES)}`, false));
	assert.equal(long[0].kind, 'code');
	assert.equal(long[0].closed, false);
});

test('an answer that was cut off keeps its unclosed block', () => {
	const [page] = artifacts(splitMessage('```html\n<p>half', true));
	assert.equal(page.kind, 'page');
	assert.equal(page.closed, false);
	assert.equal(page.code, '<p>half');
});

test('a document may hold fenced code of its own', () => {
	const doc = `# Guide\n\n${lines(14)}\n\n\`\`\`sh\nnpm install\n\`\`\`\n\nThe end.`;
	const parts = splitMessage(`Here.\n\n${fence('markdown', doc)}\n\nBye.`);
	const [d] = artifacts(parts);
	assert.equal(d.kind, 'document');
	assert.equal(d.title, 'Guide');
	assert.equal(d.code, doc);
	assert.equal(parts[parts.length - 1].text.trim(), 'Bye.');
});

test('longer and tilde fences', () => {
	const inner = '```html\n<p>not a page, an example</p>\n```';
	const parts = splitMessage(`\`\`\`\`text\n${inner}\n\`\`\`\``);
	assert.deepEqual(
		parts.map((p) => p.type),
		['text']
	);
	const [page] = artifacts(splitMessage('~~~html\n<p>x</p>\n~~~'));
	assert.equal(page.kind, 'page');
	// A fence is closed by its own mark only.
	const [open] = artifacts(splitMessage('~~~html\n<p>x</p>\n```'));
	assert.equal(open.closed, false);
	assert.equal(open.code, '<p>x</p>\n```');
});

test('inline code and indented text are not fences', () => {
	assert.deepEqual(artifacts(splitMessage('Use ```html``` like `this`.')), []);
	assert.deepEqual(artifacts(splitMessage('    ```html\n    <p>x</p>\n    ```')), []);
});

test('several artifacts in one answer are numbered', () => {
	const parts = splitMessage(`${fence('html', '<h1>A</h1>')}\ntext\n${fence('svg', '<svg></svg>')}\n${fence('sh', 'ls')}`);
	const found = parts.filter((p) => p.type === 'artifact');
	assert.deepEqual(
		found.map((p) => [p.index, p.block.kind]),
		[
			[0, 'page'],
			[1, 'drawing']
		]
	);
});

test('a conversation: ids, versions, and what is being written', () => {
	const page = (body) => fence('html', `<title>Tea timer</title>\n${body}`);
	const all = collectArtifacts([
		{ id: 1, role: 'user', content: fence('html', '<p>mine</p>') },
		{ id: 2, role: 'assistant', content: `First.\n${page('<p>one</p>')}`, state: 'done' },
		{ id: 3, role: 'user', content: 'make it blue' },
		{ id: 4, role: 'assistant', content: `Again.\n${page('<p>two</p>')}\n${fence('svg', '<svg><title>Cup</title></svg>')}`, state: 'done' },
		{ id: 5, role: 'assistant', content: '```html\n<title>Tea timer</title>\n<p>thr', state: 'streaming' },
		{ id: 6, role: 'image', content: '' }
	]);
	assert.deepEqual(
		all.map((a) => [a.id, a.kind, a.version, a.writing]),
		[
			['2:0', 'page', 1, false],
			['4:0', 'page', 2, false],
			['4:1', 'drawing', 1, false],
			['5:0', 'page', 3, true]
		]
	);
	assert.equal(all[0].group, all[1].group);
	assert.equal(all[0].group, all[3].group);
	assert.notEqual(all[0].group, all[2].group);
});

test('two blocks alike in one answer are two things', () => {
	const all = collectArtifacts([{ id: 1, role: 'assistant', content: `${fence('html', '<p>a</p>')}\n${fence('html', '<p>b</p>')}`, state: 'done' }]);
	assert.equal(all.length, 2);
	assert.notEqual(all[0].group, all[1].group);
	assert.deepEqual(
		all.map((a) => a.version),
		[1, 1]
	);
});

test('a stopped answer is not being written', () => {
	const [a] = collectArtifacts([{ id: 1, role: 'assistant', content: '```html\n<p>half', state: 'stopped' }]);
	assert.equal(a.writing, false);
	assert.equal(a.closed, false);
});

test('the page document puts the policy before anything the model wrote', () => {
	const doc = pageDocument('<!doctype html><html><head><meta http-equiv="Content-Security-Policy" content="default-src *"></head></html>');
	assert.ok(doc.startsWith('<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy"'));
	assert.ok(doc.indexOf(PAGE_CSP) < doc.indexOf('default-src *'));
	assert.ok(PAGE_CSP.startsWith("default-src 'none'"));
	assert.ok(!/https?:|connect-src|\*/.test(PAGE_CSP));
});

test('a drawing becomes an image address', () => {
	const url = svgDataUrl('<svg viewBox="0 0 10 10"><circle r="5"/></svg>');
	assert.ok(url.startsWith('data:image/svg+xml;charset=utf-8,'));
	const svg = decodeURIComponent(url.slice(url.indexOf(',') + 1));
	assert.ok(svg.startsWith('<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 10 10">'));
	const kept = decodeURIComponent(svgDataUrl('<?xml version="1.0"?>\n<svg xmlns="http://www.w3.org/2000/svg" width="4"/>').split(',')[1]);
	assert.equal(kept, '<svg xmlns="http://www.w3.org/2000/svg" width="4"/>');
	assert.equal(svgDataUrl('<p>no drawing</p>'), null);
});

test('a request for something written is told from other requests', () => {
	for (const text of ['Make a small web page titled Click counter', 'make me a website for my cafe', 'Draw a fox as SVG', 'a mermaid diagram of the login flow', 'Write a README in Markdown', 'bana bir web sayfası yap'])
		assert.ok(asksForPiece(text), text);
	for (const text of ['Draw a fox', 'Explain photosynthesis', 'Read this aloud', 'Write a poem about pages', 'What is on page 3?']) assert.ok(!asksForPiece(text), text);
});

test('file names and what is saved', () => {
	assert.equal(fileNameOf({ kind: 'page', lang: 'html', title: 'Tea & Timer!' }), 'tea-timer.html');
	assert.equal(fileNameOf({ kind: 'drawing', lang: 'svg', title: 'Çay Bardağı' }), 'cay-bardagi.svg');
	assert.equal(fileNameOf({ kind: 'document', lang: 'md', title: '日本語' }), 'document.md');
	assert.equal(fileNameOf({ kind: 'code', lang: 'python', title: 'Code' }), 'code.py');
	assert.equal(fileNameOf({ kind: 'code', lang: 'brainfuck', title: 'Code' }), 'code.txt');
	assert.equal(extensionOf({ kind: 'diagram', lang: 'mermaid' }), 'mmd');
	assert.ok(!/[\\/]/.test(fileNameOf({ kind: 'page', lang: 'html', title: '../../etc/passwd' })));
	assert.equal(textOf({ kind: 'drawing', code: '<?xml version="1.0"?>\n<svg/>' }), '<svg/>');
	assert.equal(textOf({ kind: 'page', code: '<p>x</p>' }), '<p>x</p>');
});
