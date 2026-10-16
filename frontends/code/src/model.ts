// Shapes of `oxi headless` messages, and the transcript as a flat list of items to render.

import {splitParagraphs} from './markdown.js';

export type ToolBlock = {
	type: 'tool';
	id?: string | null;
	name: string;
	kind?: string;
	title?: string;
	status: 'pending' | 'in_progress' | 'completed' | 'failed' | 'interrupted';
	args?: string | null;
	output?: string;
	output_truncated?: boolean;
	diff?: string | null;
	diff_truncated?: boolean;
	full_output_path?: string | null;
	locations?: Array<{path: string; line?: number | null}>;
};

export type Block = {type: 'thinking'; text: string} | {type: 'answer'; text: string} | ToolBlock;

export type Route = {provider: string; label: string; model: string; reason?: string; failover_from?: string | null; direct?: boolean};

export type UserMessage = {role: 'user'; text: string; summary?: boolean; attachments?: string[]; images?: unknown[]};
export type AssistantMessage = {
	role: 'assistant';
	streaming: boolean;
	blocks: Block[];
	route?: Route | null;
	worked_secs?: number | null;
};
export type Message = UserMessage | AssistantMessage;

export type Config = {provider: string; label: string; model: string; effort: string};
export type Provider = {id: string; label: string; description: string; acp: boolean; model: string};
export type Approval = {tool: string; summary: string; allow_prefix?: string | null};
export type ChatSummary = {id: string; title: string; modified: number; provider?: string | null; running: boolean; approval: boolean};

export type QuotaWindow = {label: string; used_pct: number; resets_in?: number | null; scope?: string | null};
export type Quota = {provider: string; supported: boolean; enabled: boolean; windows: QuotaWindow[]; plan?: string | null; limit_reached?: boolean; error?: string | null};

/** Something this client shows that is not a chat message: command output, notices. */
export type Notice =
	| {type: 'info'; text: string; dim?: boolean}
	| {type: 'error'; text: string}
	| {type: 'interrupted'}
	| {type: 'command'; name: string; args: string; output?: string; error?: boolean}
	| {type: 'bash'; command: string; stdout: string; stderr: string; code: number | null}
	| {type: 'lines'; lines: string[]};

export type LocalNotice = {key: string; afterMessages: number; notice: Notice};

export type Item =
	| {kind: 'user'; key: string; text: string; attachments: string[]}
	| {kind: 'text'; key: string; text: string; first: boolean}
	| {kind: 'error'; key: string; text: string}
	| {kind: 'thinking'; key: string; text: string; done: boolean}
	| {kind: 'tool'; key: string; block: ToolBlock; running: boolean}
	| {kind: 'worked'; key: string; secs: number}
	| {kind: 'notice'; key: string; notice: Notice};

type Flat = {item: Item; final: boolean};

export const isTodo = (block: ToolBlock) => /todo/i.test(block.name) || /todo/i.test(block.title ?? '');

/**
 * The transcript as items, each marked final when it will not change any more. Items of a
 * finished prefix go to the terminal's scrollback (Ink's Static); the rest are redrawn.
 */
export function flatten(messages: Array<Message | undefined>, notices: LocalNotice[]): Flat[] {
	const out: Flat[] = [];
	const addNotices = (after: number) => {
		for (const n of notices) {
			if (n.afterMessages === after) out.push({item: {kind: 'notice', key: n.key, notice: n.notice}, final: true});
		}
	};
	addNotices(0);
	messages.forEach((message, mi) => {
		if (!message) {
			addNotices(mi + 1);
			return;
		}
		if (message.role === 'user') {
			if (!message.summary) {
				out.push({
					item: {kind: 'user', key: `${mi}`, text: message.text, attachments: message.attachments ?? []},
					final: true,
				});
			}
			addNotices(mi + 1);
			return;
		}
		const streaming = message.streaming;
		message.blocks.forEach((block, bi) => {
			const last = bi === message.blocks.length - 1;
			const open = streaming && last;
			const key = `${mi}:${bi}`;
			if (block.type === 'thinking') {
				if (!block.text.trim()) return;
				out.push({item: {kind: 'thinking', key, text: block.text, done: !open}, final: !open});
			} else if (block.type === 'answer') {
				const paragraphs = splitParagraphs(block.text);
				paragraphs.forEach((paragraph, pi) => {
					const lastParagraph = pi === paragraphs.length - 1;
					const final = !(open && lastParagraph);
					const text = paragraph.replace(/\s+$/, '');
					if (!text.trim()) return;
					const error = text.match(/^\[Error\]\s*([\s\S]*)$/);
					if (error) {
						out.push({item: {kind: 'error', key: `${key}:${pi}`, text: error[1]}, final});
						return;
					}
					out.push({item: {kind: 'text', key: `${key}:${pi}`, text, first: pi === 0}, final});
				});
			} else {
				const done = block.status === 'completed' || block.status === 'failed' || block.status === 'interrupted';
				out.push({
					item: {kind: 'tool', key, block, running: !done && streaming},
					final: done || !streaming,
				});
			}
		});
		if (!streaming && message.worked_secs && message.worked_secs >= 30) {
			out.push({item: {kind: 'worked', key: `${mi}:w`, secs: message.worked_secs}, final: true});
		}
		addNotices(mi + 1);
	});
	return out;
}

/** Todo items of a `todo_write` call. */
export function todoItems(block: ToolBlock): Array<{status: string; content: string}> | null {
	if (!isTodo(block)) return null;
	try {
		const args = JSON.parse(block.args ?? '');
		if (!Array.isArray(args?.todos)) return null;
		return args.todos.map((todo: any) => ({
			status: String(todo?.status ?? 'pending'),
			content: String(todo?.content ?? todo?.activeForm ?? ''),
		}));
	} catch {
		return null;
	}
}

/** Tokens a reply has streamed so far, estimated from its length. */
export function streamedChars(message: Message | undefined): number {
	if (!message || message.role !== 'assistant') return 0;
	let chars = 0;
	for (const block of message.blocks) {
		if (block.type === 'tool') chars += (block.args?.length ?? 0);
		else chars += block.text.length;
	}
	return chars;
}
