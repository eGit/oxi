import chalk from 'chalk';
import {Box, Text, useInput} from 'ink';
import React, {useContext, useEffect, useMemo, useRef, useState} from 'react';
import * as B from '../buffer.js';
import {theme} from '../theme.js';
import {WidthContext} from './items.js';

export type Mode = 'default' | 'acceptEdits' | 'plan';
export type Command = {name: string; description: string; aliases?: string[]; args?: string};
export type Image = {mime: string; data: string};
export type Submission = {text: string; images: Image[]; bash: boolean};

type Props = {
	active: boolean;
	mode: Mode;
	history: string[];
	commands: Command[];
	files: () => string[];
	onSubmit: (submission: Submission) => void;
	onCycleMode: () => void;
	/** Esc with nothing to close; returns true when it was used (a turn was interrupted). */
	onEscape: () => boolean;
	onExitRequest: (key: 'c' | 'd') => void;
	onToggleVerbose: () => void;
	onToggleTodos: () => void;
	onPasteImage: () => Promise<Image | null>;
	notice?: string | null;
	footerRight?: React.ReactNode;
	statusLine?: React.ReactNode;
	placeholder?: string;
};

type Menu = {kind: 'command' | 'file'; query: string; start: number; items: Array<{value: string; label: string; description?: string}>};

const EXAMPLES = [
	'refactor <filepath>',
	'write a test for <filepath>',
	'how does <filepath> work?',
	'fix lint errors',
	'explain this codebase',
	'add a comment to <filepath>',
];

export function PromptInput(props: Props) {
	const {active, mode, history, commands} = props;
	const [buf, setBuf] = useState<B.Buffer>(B.empty);
	const [bashMode, setBashMode] = useState(false);
	const [menuIndex, setMenuIndex] = useState(0);
	const [dismissed, setDismissed] = useState<string | null>(null);
	const [showShortcuts, setShowShortcuts] = useState(false);
	const [escPendingAt, setEscPendingAt] = useState(0);
	const [historyIndex, setHistoryIndex] = useState(-1);
	const draft = useRef('');
	const pastes = useRef(new Map<number, string>());
	const images = useRef(new Map<number, Image>());
	const counter = useRef({paste: 0, image: 0});
	const undo = useRef<B.Buffer[]>([]);
	const pasting = useRef<string | null>(null);
	const example = useMemo(() => EXAMPLES[Math.floor(Math.random() * EXAMPLES.length)], []);

	const update = (next: B.Buffer) => {
		if (next.text !== buf.text) {
			undo.current.push(buf);
			if (undo.current.length > 200) undo.current.shift();
		}
		setBuf(next);
		setShowShortcuts(false);
	};

	const menu = useMemo((): Menu | null => {
		const before = buf.text.slice(0, buf.cursor);
		if (!bashMode && /^\/\S*$/.test(before) && !buf.text.includes('\n')) {
			const query = before.slice(1).toLowerCase();
			const starts = commands.filter(c => c.name.startsWith(query) || c.aliases?.some(a => a.startsWith(query)));
			const contains = commands.filter(c => !starts.includes(c) && c.name.includes(query));
			const items = [...starts, ...contains].map(c => ({
				value: c.name,
				label: '/' + c.name + (c.args ? ' ' + c.args : ''),
				description: c.description,
			}));
			return items.length ? {kind: 'command', query: before, start: 0, items} : null;
		}
		const at = before.match(/(^|\s)@([^\s@]*)$/);
		if (at) {
			const query = at[2];
			const items = matchFiles(props.files(), query)
				.slice(0, 5)
				.map(file => ({value: file, label: '+ ' + file}));
			return items.length ? {kind: 'file', query: before, start: before.length - query.length - 1, items} : null;
		}
		return null;
	}, [buf, bashMode, commands]);

	const openMenu = menu && menu.query !== dismissed ? menu : null;
	useEffect(() => setMenuIndex(0), [menu?.query]);

	const reset = () => {
		setBuf(B.empty);
		setBashMode(false);
		setHistoryIndex(-1);
		pastes.current.clear();
		images.current.clear();
		undo.current = [];
	};

	const expand = (text: string) =>
		text.replace(/\[Pasted text #(\d+)(?: \+\d+ lines)?\]/g, (whole, n) => pastes.current.get(Number(n)) ?? whole);

	const submit = (text: string) => {
		const used: Image[] = [];
		for (const [n, image] of images.current) {
			if (text.includes(`[Image #${n}]`)) used.push(image);
		}
		const full = expand(text).trim();
		if (!full && !used.length) return;
		props.onSubmit({text: full, images: used, bash: bashMode});
		reset();
	};

	const complete = (m: Menu, execute: boolean) => {
		const item = m.items[Math.min(menuIndex, m.items.length - 1)];
		if (m.kind === 'command') {
			if (execute) {
				submit('/' + item.value);
				return;
			}
			update(B.set('/' + item.value + ' '));
			return;
		}
		const text = buf.text.slice(0, m.start) + '@' + item.value + ' ' + buf.text.slice(buf.cursor);
		update({text, cursor: m.start + item.value.length + 2});
	};

	const insertPaste = (raw: string) => {
		const text = raw.replace(/\r\n?/g, '\n');
		const lines = text.split('\n').length;
		if (text.length > 1000 || (lines > 2 && text.length > 200)) {
			const n = ++counter.current.paste;
			pastes.current.set(n, text);
			update(B.insert(buf, `[Pasted text #${n} +${lines} lines]`));
		} else {
			update(B.insert(buf, text));
		}
	};

	useInput(
		(input, key) => {
			// Bracketed paste: everything between the markers is text, Enter included.
			if (input === '[200~') {
				pasting.current = '';
				return;
			}
			if (pasting.current !== null) {
				if (input.endsWith('[201~')) {
					const text = pasting.current + input.slice(0, -'[201~'.length).replace(/\u001b$/, '');
					pasting.current = null;
					insertPaste(text);
				} else {
					pasting.current += key.return ? '\n' : key.tab ? '\t' : input;
				}
				return;
			}

			if (key.ctrl && input === 'c') {
				if (buf.text || bashMode) reset();
				else props.onExitRequest('c');
				return;
			}
			if (key.ctrl && input === 'd') {
				if (!buf.text) props.onExitRequest('d');
				else update(B.deleteForward(buf));
				return;
			}
			if (key.ctrl && input === 'r') return props.onToggleVerbose();
			if (key.ctrl && input === 't') return props.onToggleTodos();
			if (key.ctrl && input === 'v') {
				props.onPasteImage().then(image => {
					if (!image) return;
					const n = ++counter.current.image;
					images.current.set(n, image);
					setBuf(b => B.insert(b, `[Image #${n}]`));
				});
				return;
			}
			if (key.tab && key.shift) return props.onCycleMode();

			if (key.escape) {
				if (openMenu) {
					setDismissed(openMenu.query);
					return;
				}
				if (showShortcuts) {
					setShowShortcuts(false);
					return;
				}
				if (props.onEscape()) return;
				if (!buf.text && bashMode) {
					setBashMode(false);
					return;
				}
				if (buf.text) {
					if (Date.now() - escPendingAt < 1000) {
						reset();
						setEscPendingAt(0);
					} else setEscPendingAt(Date.now());
				}
				return;
			}

			if (key.return) {
				if (key.meta || key.shift) return update(B.insert(buf, '\n'));
				if (openMenu) return complete(openMenu, openMenu.kind === 'command');
				if (buf.cursor > 0 && buf.text[buf.cursor - 1] === '\\') {
					return update(B.insert(B.backspace(buf), '\n'));
				}
				return submit(buf.text);
			}
			if (key.tab) {
				if (openMenu) complete(openMenu, false);
				return;
			}

			if (key.upArrow || key.downArrow) {
				if (openMenu) {
					const n = openMenu.items.length;
					setMenuIndex(i => (i + (key.upArrow ? -1 : 1) + n) % n);
					return;
				}
				const up = key.upArrow;
				if (up ? !B.onFirstLine(buf) : !B.onLastLine(buf)) return setBuf(B.vertical(buf, up ? -1 : 1));
				const next = historyIndex + (up ? 1 : -1);
				if (next < -1 || next >= history.length) return;
				if (historyIndex === -1) draft.current = buf.text;
				setHistoryIndex(next);
				setBuf(B.set(next === -1 ? draft.current : history[history.length - 1 - next]));
				return;
			}

			if (input === '\u001f') {
				const previous = undo.current.pop();
				if (previous) setBuf(previous);
				return;
			}
			if ((key.backspace || key.delete) && key.meta) return update(B.deleteWordBack(buf));
			if (key.backspace || key.delete) {
				if (!buf.text && bashMode) return setBashMode(false);
				return update(B.backspace(buf));
			}
			if (key.leftArrow) return setBuf(key.meta || key.ctrl ? B.wordLeft(buf) : B.left(buf));
			if (key.rightArrow) return setBuf(key.meta || key.ctrl ? B.wordRight(buf) : B.right(buf));
			if (key.home || (key.ctrl && input === 'a')) return setBuf(B.home(buf));
			if (key.end || (key.ctrl && input === 'e')) return setBuf(B.end(buf));
			if (key.ctrl && input === 'b') return setBuf(B.left(buf));
			if (key.ctrl && input === 'f') return setBuf(B.right(buf));
			if (key.ctrl && input === 'k') return update(B.killToEnd(buf));
			if (key.ctrl && input === 'u') return update(B.killToStart(buf));
			if (key.ctrl && input === 'w') return update(B.deleteWordBack(buf));
			if (key.ctrl && input === 'p') return setBuf(B.vertical(buf, -1));
			if (key.ctrl && input === 'n') return setBuf(B.vertical(buf, 1));
			if (key.meta && input === 'b') return setBuf(B.wordLeft(buf));
			if (key.meta && input === 'f') return setBuf(B.wordRight(buf));
			if (key.meta && input === 'd') return update(B.deleteWordForward(buf));
			if (key.ctrl || key.meta || !input) return;

			if (!buf.text && !bashMode && input === '!') return setBashMode(true);
			if (!buf.text && input === '?' && !bashMode) return setShowShortcuts(s => !s);
			if (input.length > 1 && /[\r\n]/.test(input)) return insertPaste(input);
			update(B.insert(buf, input.replace(/\r\n?/g, '\n')));
			setEscPendingAt(0);
		},
		{isActive: active},
	);

	const width = useContext(WidthContext);
	const borderColor = bashMode ? theme.bashBorder : theme.promptBorder;
	const content = buf.text
		? withCursor(buf.text, buf.cursor, active)
		: (active ? chalk.inverse((props.placeholder ?? `Try "${example}"`)[0]) : '') +
			chalk.rgb(153, 153, 153)((props.placeholder ?? `Try "${example}"`).slice(active ? 1 : 0));

	let left: React.ReactNode;
	if (props.notice) left = <Text color={theme.secondary}>{props.notice}</Text>;
	else if (escPendingAt && Date.now() - escPendingAt < 1000) left = <Text color={theme.secondary}>Esc again to clear</Text>;
	else if (bashMode) left = <Text color={theme.bashBorder}>! for bash mode</Text>;
	else if (mode === 'acceptEdits')
		left = (
			<Text>
				<Text color={theme.autoAccept}>⏵⏵ accept edits on</Text>
				<Text color={theme.secondary}> (shift+tab to cycle)</Text>
			</Text>
		);
	else if (mode === 'plan')
		left = (
			<Text>
				<Text color={theme.planMode}>⏸ plan mode on</Text>
				<Text color={theme.secondary}> (shift+tab to cycle)</Text>
			</Text>
		);
	else left = <Text color={theme.secondary}>? for shortcuts</Text>;

	return (
		<Box flexDirection="column" marginTop={1}>
			<Box borderStyle="round" borderLeft={false} borderRight={false} borderColor={borderColor} flexDirection="row">
				<Box width={2} flexShrink={0}>
					<Text color={bashMode ? theme.bashBorder : undefined}>{bashMode ? '!' : '>'}</Text>
				</Box>
				<Box width={Math.max(10, width - 2)}>
					<Text>{content}</Text>
				</Box>
			</Box>
			{openMenu ? (
				<MenuView menu={openMenu} index={menuIndex} />
			) : showShortcuts ? (
				<Shortcuts />
			) : (
				<Box flexDirection="column">
					<Box flexDirection="row" justifyContent="space-between" paddingX={2}>
						<Box flexShrink={1}>{left}</Box>
						<Box flexShrink={0}>{props.footerRight}</Box>
					</Box>
					{props.statusLine ? <Box paddingX={2}>{props.statusLine}</Box> : null}
				</Box>
			)}
		</Box>
	);
}

function withCursor(text: string, cursor: number, active: boolean): string {
	if (!active) return text;
	const ch = text[cursor];
	if (ch === undefined) return text + chalk.inverse(' ');
	if (ch === '\n') return text.slice(0, cursor) + chalk.inverse(' ') + text.slice(cursor);
	const code = text.codePointAt(cursor)!;
	const width = code > 0xffff ? 2 : 1;
	return text.slice(0, cursor) + chalk.inverse(text.slice(cursor, cursor + width)) + text.slice(cursor + width);
}

function MenuView({menu, index}: {menu: Menu; index: number}) {
	const width = Math.max(...menu.items.map(i => i.label.length)) + 4;
	const start = Math.max(0, Math.min(index - 4, menu.items.length - 8));
	return (
		<Box flexDirection="column" paddingX={2}>
			{menu.items.slice(start, start + 8).map((item, i) => {
				const selected = start + i === index;
				return (
					<Text key={item.value} wrap="truncate-end">
						<Text color={selected ? theme.suggestion : theme.secondary}>{item.description ? item.label.padEnd(width) : item.label}</Text>
						{item.description ? <Text color={selected ? theme.suggestion : theme.secondary}>{item.description}</Text> : null}
					</Text>
				);
			})}
		</Box>
	);
}

function Shortcuts() {
	const columns = [
		['! for bash mode', '/ for commands', '@ for file paths'],
		['double tap esc to clear input', 'shift + tab to cycle modes', 'ctrl + r for verbose output'],
		['ctrl + t to toggle todos', '\\⏎ for newline', 'ctrl + v to paste images'],
	];
	return (
		<Box flexDirection="row" paddingX={2} gap={4}>
			{columns.map((column, i) => (
				<Box key={i} flexDirection="column">
					{column.map(line => (
						<Text key={line} color={theme.secondary}>
							{line}
						</Text>
					))}
				</Box>
			))}
		</Box>
	);
}

/** Paths matching what follows `@`: the letters in order, file names and starts ranked first. */
export function matchFiles(files: string[], query: string): string[] {
	if (!query) return files.slice(0, 50);
	const q = query.toLowerCase();
	const scored: Array<[number, string]> = [];
	for (const file of files) {
		const lower = file.toLowerCase();
		const name = lower.slice(lower.lastIndexOf('/') + 1);
		let score: number;
		if (name.startsWith(q)) score = 0;
		else if (lower.startsWith(q)) score = 1;
		else if (name.includes(q)) score = 2;
		else if (lower.includes(q)) score = 3;
		else {
			let at = 0;
			for (const ch of lower) if (ch === q[at]) at++;
			if (at < q.length) continue;
			score = 4;
		}
		scored.push([score * 1000 + file.length, file]);
	}
	return scored.sort((a, b) => a[0] - b[0]).map(([, file]) => file);
}
