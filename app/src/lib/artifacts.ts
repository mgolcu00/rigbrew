// Artifacts: things the chat model wrote to be looked at or kept rather than read as a reply
// (a web page, a drawing, a diagram, a document, a longer piece of code). They are found in the
// text of an answer, as fenced code blocks, so nothing extra is stored with a chat.
//
// Everything here is plain functions over text, with no imports, so it can be tested with
// `node --test src/lib/artifacts.test.mjs`.

export type ArtifactKind = 'page' | 'drawing' | 'diagram' | 'document' | 'code';

/** One fenced block that is shown as an artifact. */
export interface Block {
	kind: ArtifactKind;
	/** The language the fence names, lower case; '' when it names none. */
	lang: string;
	code: string;
	/** False while the closing fence has not been written (the answer is still arriving, or was cut off). */
	closed: boolean;
	title: string;
	lines: number;
}

export type Part = { type: 'text'; text: string } | { type: 'artifact'; index: number; block: Block };

/** A Markdown document shorter than this stays in the answer. */
export const DOCUMENT_MIN_LINES = 16;
/** Code in any other language shorter than this stays in the answer. */
export const CODE_MIN_LINES = 26;

/** Said to the chat model on every turn, so it writes things the app can show this way. */
export const artifactsPrompt =
	'When the user asks you to make a web page, a document, a diagram, or a drawing as SVG, write the whole thing in one fenced code block with the right language tag (html, markdown, mermaid or svg) ' +
	'and keep the text around it to a sentence or two. The app shows that block to the user as a finished piece next to the chat.';

/**
 * Whether a message plainly asks for something that is written rather than made by a tool: a
 * web page, an SVG, a diagram, a Markdown document. A small model offered a picture tool
 * reaches for it on "make a web page"; on such a turn the tools are left out.
 */
export const asksForPiece = (text: string) =>
	/\b(web ?pages?|web ?sites?|landing pages?|html|css|svg|markdown|mermaid|flow ?charts?|diagrams?|readme)\b/i.test(text) || /(web sayfa|web site|diyagram|akış şema)/i.test(text);

const names: Record<ArtifactKind, string> = { page: 'Page', drawing: 'Drawing', diagram: 'Diagram', document: 'Document', code: 'Code' };

/** "Page", "Drawing", "Diagram", "Document" or "Code". */
export const kindName = (kind: ArtifactKind) => names[kind];

const countLines = (code: string) => (code === '' ? 0 : code.split('\n').length);

const startsWithSvg = (code: string) => /^\s*(<\?xml[^>]*\?>\s*)?(<!--[\s\S]*?-->\s*)*(<!doctype[^>]*>\s*)?<svg[\s/>]/i.test(code);

/** What a fenced block is shown as, or null when it stays in the answer as it is today. */
export function kindOf(lang: string, code: string): ArtifactKind | null {
	const name = lang.toLowerCase();
	if (name === 'svg') return 'drawing';
	if (name === 'html' || name === 'htm' || name === 'xhtml') return startsWithSvg(code) ? 'drawing' : 'page';
	if (name === 'xml' && startsWithSvg(code)) return 'drawing';
	if (name === 'mermaid') return 'diagram';
	const lines = countLines(code);
	if (name === 'markdown' || name === 'md') return lines >= DOCUMENT_MIN_LINES ? 'document' : null;
	return lines >= CODE_MIN_LINES ? 'code' : null;
}

const entities: Record<string, string> = { amp: '&', lt: '<', gt: '>', quot: '"', apos: "'", nbsp: ' ' };

function plain(html: string): string {
	return html
		.replace(/<[^>]*>/g, '')
		.replace(/&(#x?[0-9a-f]+|[a-z]+);/gi, (all, name: string) => {
			if (name[0] !== '#') return entities[name.toLowerCase()] ?? all;
			const code = name[1].toLowerCase() === 'x' ? parseInt(name.slice(2), 16) : parseInt(name.slice(1), 10);
			return Number.isFinite(code) && code > 0 && code <= 0x10ffff ? String.fromCodePoint(code) : all;
		})
		.replace(/\s+/g, ' ')
		.trim();
}

const clip = (title: string) => ([...title].length > 80 ? `${[...title].slice(0, 79).join('')}…` : title);

/** The first `<title>`, `<h1>` or Markdown heading, else the kind's own name. */
export function titleOf(kind: ArtifactKind, code: string): string {
	let found = '';
	if (kind === 'page') {
		found = plain(code.match(/<title[^>]*>([\s\S]*?)<\/title>/i)?.[1] ?? '') || plain(code.match(/<h1[^>]*>([\s\S]*?)<\/h1>/i)?.[1] ?? '');
	} else if (kind === 'drawing') {
		found = plain(code.match(/<title[^>]*>([\s\S]*?)<\/title>/i)?.[1] ?? '');
	} else if (kind === 'document') {
		const heading = code.match(/^ {0,3}#{1,6}[ \t]+(.+?)[ \t]*#*[ \t]*$/m)?.[1] ?? '';
		found = heading
			.replace(/!?\[([^\]]*)\]\([^)]*\)/g, '$1')
			.replace(/[*_`~]/g, '')
			.replace(/\s+/g, ' ')
			.trim();
	}
	return clip(found) || names[kind];
}

interface Fence {
	mark: string;
	length: number;
	lang: string;
	/** The line the fence opened on, as written. */
	opener: string;
	body: string[];
	/** Fences opened inside a Markdown document (a document may show code of its own). */
	inner: number;
}

const OPEN = /^ {0,3}(`{3,}|~{3,})[ \t]*([^\s`]*)(.*)$/;

function opens(line: string): { mark: string; length: number; lang: string } | null {
	const m = OPEN.exec(line);
	if (!m) return null;
	// A backtick fence cannot have a backtick in what follows it.
	if (m[1][0] === '`' && m[3].includes('`')) return null;
	return { mark: m[1][0], length: m[1].length, lang: m[2].toLowerCase() };
}

function closes(line: string, fence: Fence): boolean {
	const m = /^ {0,3}(`{3,}|~{3,})[ \t]*$/.exec(line);
	return Boolean(m && m[1][0] === fence.mark && m[1].length >= fence.length);
}

/**
 * Cuts an answer into what is read (Markdown text) and what is shown as an artifact. Short code
 * stays inside the text, fences and all, and is rendered the way it always was. `done` is false
 * while the answer is still being written: a block that is open at the end is then an artifact
 * as soon as it can be told to be one.
 */
export function splitMessage(text: string, done = true): Part[] {
	const parts: Part[] = [];
	let prose: string[] = [];
	let fence: Fence | null = null;
	let index = 0;

	const flush = () => {
		const said = prose.join('\n');
		if (said.trim()) parts.push({ type: 'text', text: said });
		prose = [];
	};
	const end = (closer: string | null) => {
		const f = fence!;
		fence = null;
		const code = f.body.join('\n');
		const kind = kindOf(f.lang, code);
		if (!kind) {
			prose.push(f.opener, ...f.body);
			if (closer !== null) prose.push(closer);
			return;
		}
		flush();
		parts.push({ type: 'artifact', index: index++, block: { kind, lang: f.lang, code, closed: closer !== null, title: titleOf(kind, code), lines: countLines(code) } });
	};

	for (const line of text.split('\n')) {
		if (!fence) {
			const o = opens(line);
			if (o) fence = { ...o, opener: line, body: [], inner: 0 };
			else prose.push(line);
			continue;
		}
		// A Markdown document may hold fenced code of its own: a fence with a language opens
		// one, the next bare fence closes it, and only after that does a bare fence end the document.
		if (fence.lang === 'markdown' || fence.lang === 'md') {
			if (fence.inner > 0) {
				if (closes(line, fence)) fence.inner--;
				fence.body.push(line);
				continue;
			}
			const o = opens(line);
			if (o && o.lang && o.mark === fence.mark && o.length <= fence.length) {
				fence.inner++;
				fence.body.push(line);
				continue;
			}
		}
		if (closes(line, fence)) end(line);
		else fence.body.push(line);
	}
	if (fence) {
		// The answer ended inside a block. While it is being written the last line may be half
		// of the closing fence: it is not shown as code.
		const f: Fence = fence;
		if (!done && f.body.length > 0 && /^ {0,3}[`~]{1,}$/.test(f.body[f.body.length - 1]) && f.body[f.body.length - 1].trim()[0] === f.mark) f.body.pop();
		end(null);
	}
	flush();
	return parts;
}

/** An artifact in a conversation: a block, where it is, and which version of what it is. */
export interface Artifact extends Block {
	/** `<message id>:<place in the message>`. */
	id: string;
	messageId: number;
	index: number;
	/** What it is a version of: artifacts of one kind with one title are the same thing, rewritten. */
	group: string;
	/** 1 for the first, counted through the conversation. */
	version: number;
	/** True while the answer it is in is still being written and the block is not closed. */
	writing: boolean;
}

interface Said {
	id: number;
	role: string;
	content: string;
	state?: string;
}

/** Every artifact in a conversation, in the order written, with versions counted. */
export function collectArtifacts(messages: readonly Said[]): Artifact[] {
	const out: Artifact[] = [];
	const versions = new Map<string, number>();
	for (const m of messages) {
		if (m.role !== 'assistant' || !m.content) continue;
		const streaming = m.state === 'streaming';
		const inMessage = new Map<string, number>();
		for (const part of splitMessage(m.content, !streaming)) {
			if (part.type !== 'artifact') continue;
			const { block } = part;
			const base = `${block.kind}:${block.kind === 'code' ? block.lang : ''}:${block.title.toLowerCase()}`;
			// Two blocks alike in one answer are two things, not two versions of one.
			const nth = inMessage.get(base) ?? 0;
			inMessage.set(base, nth + 1);
			const group = `${base}:${nth}`;
			const version = (versions.get(group) ?? 0) + 1;
			versions.set(group, version);
			out.push({ ...block, id: `${m.id}:${part.index}`, messageId: m.id, index: part.index, group, version, writing: streaming && !block.closed });
		}
	}
	return out;
}

/** What a page shown in the app may do: its own styles and scripts, pictures it carries, nothing from anywhere else. */
export const PAGE_CSP = "default-src 'none'; style-src 'unsafe-inline'; script-src 'unsafe-inline'; img-src data: blob:";

/**
 * A page as the document that goes into the sandboxed frame. The policy comes first, before
 * anything the model wrote, so nothing in the page can come ahead of it.
 */
export function pageDocument(code: string): string {
	return `<!doctype html><meta charset="utf-8"><meta http-equiv="Content-Security-Policy" content="${PAGE_CSP}">\n${code}`;
}

/**
 * A drawing as an address for `<img>`. Shown that way it cannot run script or load anything,
 * and it never becomes part of the app's own page. Null when there is no `<svg>` in it.
 */
export function svgDataUrl(code: string): string | null {
	const at = code.search(/<svg[\s/>]/i);
	if (at < 0) return null;
	let svg = code.slice(at);
	const tag = svg.slice(0, svg.indexOf('>') + 1);
	if (tag && !/\sxmlns\s*=/.test(tag)) svg = `<svg xmlns="http://www.w3.org/2000/svg"${svg.slice(4)}`;
	return `data:image/svg+xml;charset=utf-8,${encodeURIComponent(svg)}`;
}

const extensions: Record<string, string> = {
	javascript: 'js',
	js: 'js',
	jsx: 'jsx',
	typescript: 'ts',
	ts: 'ts',
	tsx: 'tsx',
	python: 'py',
	py: 'py',
	rust: 'rs',
	rs: 'rs',
	go: 'go',
	swift: 'swift',
	kotlin: 'kt',
	java: 'java',
	c: 'c',
	cpp: 'cpp',
	'c++': 'cpp',
	csharp: 'cs',
	cs: 'cs',
	ruby: 'rb',
	php: 'php',
	bash: 'sh',
	sh: 'sh',
	shell: 'sh',
	zsh: 'sh',
	sql: 'sql',
	json: 'json',
	yaml: 'yaml',
	yml: 'yaml',
	toml: 'toml',
	css: 'css',
	xml: 'xml',
	dart: 'dart',
	lua: 'lua',
	svelte: 'svelte',
	vue: 'vue'
};

/** The ending a saved artifact gets. */
export function extensionOf(block: Pick<Block, 'kind' | 'lang'>): string {
	if (block.kind === 'page') return 'html';
	if (block.kind === 'drawing') return 'svg';
	if (block.kind === 'document') return 'md';
	if (block.kind === 'diagram') return 'mmd';
	return extensions[block.lang] ?? 'txt';
}

/** A file name for saving: the title in lower case with dashes, and the kind's ending. */
export function fileNameOf(block: Pick<Block, 'kind' | 'lang' | 'title'>): string {
	const stem = block.title
		.normalize('NFKD')
		.replace(/[̀-ͯ]/g, '')
		.toLowerCase()
		.replace(/ı/g, 'i')
		.replace(/ß/g, 'ss')
		.replace(/[^a-z0-9]+/g, '-')
		.replace(/^-+|-+$/g, '')
		.slice(0, 48)
		.replace(/-+$/, '');
	return `${stem || names[block.kind].toLowerCase()}.${extensionOf(block)}`;
}

/** What is saved or copied: the code as written; a drawing starts at its `<svg>`. */
export function textOf(block: Pick<Block, 'kind' | 'code'>): string {
	if (block.kind !== 'drawing') return block.code;
	const at = block.code.search(/<svg[\s/>]/i);
	return at > 0 ? block.code.slice(at) : block.code;
}
