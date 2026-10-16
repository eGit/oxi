// How a tool call reads in the transcript, Claude Code style: `Name(args)` on the first line,
// a one-line result under it.

import path from 'node:path';
import type {ToolBlock} from './model.js';

const NAMES: Record<string, string> = {
	bash: 'Bash',
	terminal: 'Bash',
	read: 'Read',
	read_file: 'Read',
	write: 'Write',
	write_file: 'Write',
	edit: 'Update',
	multiedit: 'Update',
	multi_edit: 'Update',
	str_replace: 'Update',
	grep: 'Search',
	find: 'Search',
	glob: 'Search',
	codebase_search: 'Search',
	ls: 'List',
	web_search: 'Web Search',
	websearch: 'Web Search',
	web_fetch: 'Fetch',
	webfetch: 'Fetch',
	task: 'Task',
	agent: 'Task',
	delete: 'Delete',
	move: 'Move',
	mkdir: 'Create',
	git_status: 'Git Status',
	git_diff: 'Git Diff',
	diagnostics: 'Diagnostics',
	scratchpad: 'Scratchpad',
	notebookedit: 'Edit Notebook',
	exitplanmode: 'Plan',
	exit_plan_mode: 'Plan',
};

const KINDS: Record<string, string> = {
	execute: 'Bash',
	read: 'Read',
	edit: 'Update',
	search: 'Search',
	fetch: 'Fetch',
	delete: 'Delete',
	move: 'Move',
	think: 'Task',
};

export type ToolView = {
	/** `Bash`, `Read`, `Update`, or the tool's own name. */
	name: string;
	/** What goes in the parentheses; empty for none. */
	args: string;
	/** Group: decides what the result line says. */
	family: 'bash' | 'read' | 'write' | 'edit' | 'search' | 'list' | 'fetch' | 'websearch' | 'task' | 'todo' | 'other';
};

export function parseArgs(block: ToolBlock): Record<string, any> | null {
	if (!block.args) return null;
	try {
		const value = JSON.parse(block.args);
		return value && typeof value === 'object' && !Array.isArray(value) ? value : null;
	} catch {
		return null;
	}
}

export function relativePath(file: string, root: string): string {
	if (!file) return file;
	if (path.isAbsolute(file)) {
		const rel = path.relative(root, file);
		if (rel && !rel.startsWith('..') && !path.isAbsolute(rel)) return rel;
	}
	return file;
}

const firstLine = (text: string) => text.split('\n')[0] ?? '';

export function toolView(block: ToolBlock, root: string): ToolView {
	const raw = block.name || 'tool';
	const lower = raw.toLowerCase();
	const args = parseArgs(block) ?? {};

	const mcp = raw.match(/^mcp__(.+?)__(.+)$/);
	if (mcp && !/todo/i.test(mcp[2])) {
		const shown = Object.entries(args)
			.slice(0, 3)
			.map(([key, value]) => `${key}: ${JSON.stringify(value)}`)
			.join(', ');
		return {name: `${mcp[1]} - ${mcp[2]} (MCP)`, args: truncate(shown, 160), family: 'other'};
	}
	if (/todo/i.test(raw) || /todo/i.test(block.title ?? '')) return {name: 'Update Todos', args: '', family: 'todo'};

	const name = NAMES[lower] ?? KINDS[block.kind ?? ''] ?? titleCase(raw);
	const file = String(args.file_path ?? args.path ?? args.notebook_path ?? '');
	const title = cleanTitle(block.title ?? '', name);

	switch (name) {
		case 'Bash':
			return {name, args: truncate(firstLine(String(args.command ?? title)), 160), family: 'bash'};
		case 'Read': {
			return {name, args: relativePath(file || title, root), family: 'read'};
		}
		case 'Write':
			return {name, args: relativePath(file || title, root), family: 'write'};
		case 'Update':
			return {name, args: relativePath(file || title, root), family: 'edit'};
		case 'Search': {
			const parts: string[] = [];
			const pattern = args.pattern ?? args.query ?? args.glob;
			if (pattern !== undefined) parts.push(`${args.query !== undefined && args.pattern === undefined ? 'query' : 'pattern'}: "${pattern}"`);
			if (args.path) parts.push(`path: "${relativePath(String(args.path), root)}"`);
			if (args.glob && args.pattern !== undefined) parts.push(`glob: "${args.glob}"`);
			return {name, args: truncate(parts.join(', ') || title, 160), family: 'search'};
		}
		case 'List':
			return {name, args: relativePath(file || title || '.', root), family: 'list'};
		case 'Fetch':
			return {name, args: String(args.url ?? title), family: 'fetch'};
		case 'Web Search':
			return {name, args: `"${args.query ?? title}"`, family: 'websearch'};
		case 'Task':
			return {name, args: truncate(String(args.description ?? title), 120), family: 'task'};
		case 'Move':
			return {name, args: [args.from, args.to].filter(Boolean).map(p => relativePath(String(p), root)).join(' → ') || title, family: 'other'};
		default:
			return {name, args: truncate(file ? relativePath(file, root) : title, 160), family: 'other'};
	}
}

/** ACP titles repeat the tool ("Read src/x.rs", "`ls`"); keep what is about the call. */
function cleanTitle(title: string, name: string): string {
	let t = firstLine(title.trim());
	if (t.startsWith('`') && t.endsWith('`') && t.length > 1) t = t.slice(1, -1);
	if (/^(read|edit|write) file$/i.test(t)) return '';
	for (const word of [name, 'Read', 'Edit', 'Write', 'Run', 'Terminal']) {
		if (t.toLowerCase().startsWith(word.toLowerCase() + ' ')) {
			t = t.slice(word.length + 1);
			break;
		}
	}
	return t.replace(/^`|`$/g, '');
}

function titleCase(name: string): string {
	return name
		.replace(/[_-]+/g, ' ')
		.replace(/\b\w/g, ch => ch.toUpperCase())
		.trim();
}

export function truncate(text: string, max: number): string {
	return text.length > max ? text.slice(0, max - 1) + '…' : text;
}

export type DiffLine = {type: 'add' | 'remove' | 'context' | 'gap'; text: string; line?: number};

/**
 * Lines of a unified diff with the line number each one has in its file. ACP agents send
 * diffs of the edited snippet, numbered from 1; `offset` moves them to where it is in the file.
 */
export function parseDiff(diff: string, offset = 0): {lines: DiffLine[]; added: number; removed: number} {
	const lines: DiffLine[] = [];
	let added = 0;
	let removed = 0;
	let oldLine = 0;
	let newLine = 0;
	let hunks = 0;
	const rows = diff.split('\n');
	if (rows[rows.length - 1] === '') rows.pop();
	for (const raw of rows) {
		const hunk = raw.match(/^@@ -(\d+)(?:,\d+)? \+(\d+)(?:,\d+)? @@/);
		// Headers (---, +++, diff, index) come before the first hunk.
		if (!hunk && (hunks === 0 || raw.startsWith('\\'))) continue;
		if (hunk) {
			if (hunks++ > 0) lines.push({type: 'gap', text: '...'});
			oldLine = Number(hunk[1]) + offset;
			newLine = Number(hunk[2]) + offset;
			continue;
		}
		if (raw.startsWith('+')) {
			lines.push({type: 'add', text: raw.slice(1), line: newLine++});
			added++;
		} else if (raw.startsWith('-')) {
			lines.push({type: 'remove', text: raw.slice(1), line: oldLine++});
			removed++;
		} else if (raw.startsWith(' ') || raw === '') {
			lines.push({type: 'context', text: raw.slice(1), line: newLine++});
			oldLine++;
		}
	}
	return {lines, added, removed};
}

/**
 * A unified diff of two snippets (an edit's old and new text) by longest common subsequence,
 * numbered from `startLine`, so a proposed edit reads like the diff of the file.
 */
export function snippetDiff(before: string, after: string, startLine = 1): string {
	const a = before.replace(/\n$/, '').split('\n');
	const b = after.replace(/\n$/, '').split('\n');
	if (a.length * b.length > 1_000_000) {
		return `@@ -${startLine} +${startLine} @@\n` + [...a.map(l => '-' + l), ...b.map(l => '+' + l)].join('\n');
	}
	const dp = Array.from({length: a.length + 1}, () => new Array<number>(b.length + 1).fill(0));
	for (let i = a.length - 1; i >= 0; i--) {
		for (let j = b.length - 1; j >= 0; j--) {
			dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
		}
	}
	const out: string[] = [];
	let i = 0;
	let j = 0;
	while (i < a.length || j < b.length) {
		if (i < a.length && j < b.length && a[i] === b[j]) {
			out.push(' ' + a[i++]);
			j++;
		} else if (i < a.length && (j >= b.length || dp[i + 1][j] >= dp[i][j + 1])) {
			out.push('-' + a[i++]);
		} else {
			out.push('+' + b[j++]);
		}
	}
	return `@@ -${startLine},${a.length} +${startLine},${b.length} @@\n` + out.join('\n');
}

/** Where an agent's snippet diff sits in the file: its first line is the edit's location. */
export function diffOffset(block: ToolBlock): number {
	const line = block.locations?.find(l => l.line)?.line;
	const first = block.diff?.match(/^@@ -1(?:,\d+)? \+1(?:,\d+)? @@/m);
	return line && line > 1 && first ? line - 1 : 0;
}

export const plural = (n: number, word: string, many = word + 's') => `${n} ${n === 1 ? word : many}`;

/** The `⎿` line under a finished call. */
export function resultSummary(block: ToolBlock, view: ToolView, root: string): string | null {
	const output = (block.output ?? '').replace(/\s+$/, '');
	// Agents fence what they read; the fences are not lines of the file.
	const count = output ? output.split('\n').filter(line => !/^```/.test(line)).length : 0;
	switch (view.family) {
		case 'read':
			return `Read ${plural(count, 'line')}`;
		case 'write': {
			const {added} = block.diff ? parseDiff(block.diff) : {added: count};
			return `Wrote ${plural(added, 'line')} to ${view.args}`;
		}
		case 'edit': {
			if (!block.diff) return output ? firstLine(output) : `Updated ${view.args}`;
			const {added, removed} = parseDiff(block.diff);
			const parts = [];
			if (added) parts.push(`${added} ${added === 1 ? 'addition' : 'additions'}`);
			if (removed) parts.push(`${removed} ${removed === 1 ? 'removal' : 'removals'}`);
			return `Updated ${relativePath(view.args, root)}${parts.length ? ' with ' + parts.join(' and ') : ''}`;
		}
		case 'search':
			if (!output || /^no (matches|files|results)/i.test(output)) return 'Found 0 results';
			return `Found ${plural(count, block.name === 'find' || block.name === 'glob' ? 'file' : 'line')}`;
		case 'list':
			return `Listed ${plural(count, 'path')}`;
		case 'fetch':
			return `Received ${formatBytes(Buffer.byteLength(block.output ?? ''))}`;
		case 'websearch':
			return 'Did 1 search';
		case 'task':
			return 'Done';
		default:
			return null;
	}
}

export function formatBytes(n: number): string {
	if (n < 1024) return `${n} bytes`;
	if (n < 1024 * 1024) return `${(n / 1024).toFixed(1)}KB`;
	return `${(n / 1024 / 1024).toFixed(1)}MB`;
}
