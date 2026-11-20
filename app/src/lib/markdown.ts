import DOMPurify from 'dompurify';
import { marked } from 'marked';

const escape = (text: string) => text.replace(/[&<>"']/g, (c) => ({ '&': '&amp;', '<': '&lt;', '>': '&gt;', '"': '&quot;', "'": '&#39;' })[c]!);

marked.setOptions({ gfm: true, breaks: true });

// A code block gets a head line: the language it names, and a copy action. The transcript
// handles the click (`data-copy`), so nothing here runs script.
marked.use({
	renderer: {
		code({ text, lang }) {
			const name = (lang ?? '').trim().split(/\s+/)[0];
			return `<div class="code"><div class="code-head"><span>${escape(name || 'text')}</span><button type="button" data-copy>Copy</button></div><pre><code>${escape(text)}</code></pre></div>`;
		}
	}
});

/**
 * Model output is untrusted: render Markdown, then strip anything that is not plain content.
 * `breaks: false` for hand-wrapped documents (the legal texts), where a line end is not a break.
 */
export function renderMarkdown(text: string, breaks = true): string {
	const html = marked.parse(text, { async: false, breaks }) as string;
	return DOMPurify.sanitize(html, { FORBID_TAGS: ['style', 'form', 'input', 'img', 'video', 'audio', 'iframe'], FORBID_ATTR: ['style'] });
}
