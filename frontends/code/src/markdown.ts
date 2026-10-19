// Markdown to ANSI text, styled like Claude Code: no markup shown, code highlighted, inline
// code in the permission color. Ink wraps the result to the terminal width.

import chalk from 'chalk';
import {marked, type Token, type Tokens} from 'marked';
import stringWidth from 'string-width';
import {highlight} from './highlight.js';
import {c} from './theme.js';

const EOL = '\n';

export function renderMarkdown(text: string): string {
	const tokens = marked.lexer(text.replace(/\r\n/g, '\n'), {gfm: true});
	return tokens
		.map(token => renderBlock(token, 0))
		.join('')
		.replace(/\n+$/, '');
}

function renderBlock(token: Token, depth: number): string {
	switch (token.type) {
		case 'heading': {
			const t = token as Tokens.Heading;
			const inline = renderInline(t.tokens);
			const styled = t.depth === 1 ? chalk.bold.italic.underline(inline) : chalk.bold(inline);
			return styled + EOL + EOL;
		}
		case 'paragraph':
			return renderInline((token as Tokens.Paragraph).tokens) + EOL + EOL;
		case 'text': {
			const t = token as Tokens.Text;
			return (t.tokens ? renderInline(t.tokens) : t.text) + EOL;
		}
		case 'code': {
			const t = token as Tokens.Code;
			return highlight(t.text, t.lang?.split(/\s/)[0]) + EOL + EOL;
		}
		case 'blockquote': {
			const t = token as Tokens.Blockquote;
			const body = t.tokens.map(child => renderBlock(child, depth)).join('').replace(/\n+$/, '');
			return (
				body
					.split(EOL)
					.map(line => c.secondary('▎ ') + chalk.italic(c.secondary(line)))
					.join(EOL) + EOL + EOL
			);
		}
		case 'list': {
			const t = token as Tokens.List;
			const start = typeof t.start === 'number' ? t.start : 1;
			const items = t.items.map((item, i) => renderListItem(item, depth, t.ordered ? start + i : undefined));
			return items.join('') + (depth === 0 ? EOL : '');
		}
		case 'table':
			return renderTable(token as Tokens.Table) + EOL + EOL;
		case 'hr':
			return '---' + EOL + EOL;
		case 'space':
			return '';
		case 'html':
			return (token as Tokens.HTML).text + EOL;
		default:
			return 'raw' in token ? String((token as any).raw) : '';
	}
}

function renderListItem(item: Tokens.ListItem, depth: number, number?: number): string {
	const indent = '  '.repeat(depth);
	let marker = number !== undefined ? `${number}. ` : '- ';
	if (item.task) marker += item.checked ? '[x] ' : '[ ] ';
	const pad = ' '.repeat(marker.length);
	let out = '';
	let first = true;
	for (const child of item.tokens) {
		if (child.type === 'list') {
			out += renderBlock(child, depth + 1);
			continue;
		}
		const body = renderBlock(child, depth).replace(/\n+$/, '');
		if (!body) continue;
		const lines = body.split(EOL);
		out += lines
			.map((line, i) => (first && i === 0 ? indent + marker : indent + pad) + line)
			.join(EOL) + EOL;
		first = false;
	}
	return out || indent + marker + EOL;
}

function renderInline(tokens: Token[] | undefined): string {
	if (!tokens) return '';
	return tokens.map(renderInlineToken).join('');
}

function renderInlineToken(token: Token): string {
	switch (token.type) {
		case 'strong':
			return chalk.bold(renderInline((token as Tokens.Strong).tokens));
		case 'em':
			return chalk.italic(renderInline((token as Tokens.Em).tokens));
		case 'del':
			return chalk.strikethrough(renderInline((token as Tokens.Del).tokens));
		case 'codespan':
			return c.permission(unescape((token as Tokens.Codespan).text));
		case 'link': {
			const t = token as Tokens.Link;
			const label = renderInline(t.tokens);
			const plain = t.tokens?.map(x => ('text' in x ? x.text : '')).join('') ?? '';
			return plain === t.href || t.href.startsWith('mailto:') ? c.suggestion(label) : `${label} (${c.suggestion(t.href)})`;
		}
		case 'image': {
			const t = token as Tokens.Image;
			return `[Image: ${t.text || t.href}]`;
		}
		case 'br':
			return EOL;
		case 'text': {
			const t = token as Tokens.Text;
			return t.tokens ? renderInline(t.tokens) : unescape(t.text);
		}
		case 'escape':
			return (token as Tokens.Escape).text;
		case 'html':
			return (token as Tokens.HTML).text;
		default:
			return 'text' in token ? unescape(String((token as any).text)) : '';
	}
}

function unescape(text: string): string {
	return text
		.replace(/&amp;/g, '&')
		.replace(/&lt;/g, '<')
		.replace(/&gt;/g, '>')
		.replace(/&quot;/g, '"')
		.replace(/&#39;/g, "'");
}

function renderTable(table: Tokens.Table): string {
	const header = table.header.map(cell => renderInline(cell.tokens));
	const rows = table.rows.map(row => row.map(cell => renderInline(cell.tokens)));
	const widths = header.map((cell, i) => Math.max(stringWidth(cell), ...rows.map(row => stringWidth(row[i] ?? ''))));
	const pad = (text: string, width: number, align: string | null) => {
		const space = Math.max(0, width - stringWidth(text));
		if (align === 'right') return ' '.repeat(space) + text;
		if (align === 'center') return ' '.repeat(Math.floor(space / 2)) + text + ' '.repeat(Math.ceil(space / 2));
		return text + ' '.repeat(space);
	};
	const line = (left: string, mid: string, right: string) =>
		left + widths.map(width => '─'.repeat(width + 2)).join(mid) + right;
	const row = (cells: string[], bold = false) =>
		'│' +
		cells
			.map((cell, i) => ' ' + pad(bold ? chalk.bold(cell) : cell, widths[i], table.align[i]) + ' ')
			.join('│') +
		'│';
	return [
		line('┌', '┬', '┐'),
		row(header, true),
		line('├', '┼', '┤'),
		...rows.map(cells => row(cells)),
		line('└', '┴', '┘'),
	].join(EOL);
}

/**
 * Split streaming Markdown into the paragraphs that are complete and the one still being
 * written: a paragraph ends at a blank line outside a code fence.
 */
export function splitParagraphs(text: string): string[] {
	const lines = text.replace(/\r\n/g, '\n').split('\n');
	const parts: string[] = [];
	let current: string[] = [];
	let fence: string | null = null;
	// The text's final newline only ends a line; a paragraph ends at an actual blank line.
	if (lines.length > 1 && lines[lines.length - 1] === '') lines.pop();
	for (const line of lines) {
		const marker = line.match(/^\s*(```+|~~~+)/)?.[1];
		if (marker) {
			if (fence === null) fence = marker[0];
			else if (marker[0] === fence) fence = null;
		}
		if (fence === null && line.trim() === '' && current.some(l => l.trim() !== '')) {
			parts.push(current.join('\n'));
			current = [];
			continue;
		}
		if (line.trim() === '' && current.length === 0) continue;
		current.push(line);
	}
	parts.push(current.join('\n'));
	return parts;
}
