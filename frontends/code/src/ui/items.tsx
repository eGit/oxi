import chalk from 'chalk';
import {Box, Text} from 'ink';
import React, {createContext, useContext} from 'react';
import stringWidth from 'string-width';
import {highlight, languageForPath} from '../highlight.js';
import {renderMarkdown} from '../markdown.js';
import type {Item, Notice, ToolBlock} from '../model.js';
import {todoItems} from '../model.js';
import {theme} from '../theme.js';
import {diffOffset, parseArgs, parseDiff, plural, resultSummary, snippetDiff, toolView, type DiffLine} from '../tools.js';
import {formatDuration} from './format.js';

export type ItemOptions = {
	root: string;
	columns: number;
	verbose: boolean;
	/** Blink phase for running tool dots. */
	blink?: boolean;
	/** Dynamic text is cut to its last lines so the live area never outgrows the screen. */
	maxLines?: number;
};

/**
 * Width the transcript lays out in. Text boxes get it explicitly: a word longer than the
 * line (a path) would otherwise widen its box past the screen instead of breaking.
 */
export const WidthContext = createContext(80);

/** `⎿` lines under a heading: the first gets the elbow, the rest line up after it. */
export function Result({children, color, dim}: {children: React.ReactNode; color?: string; dim?: boolean}) {
	const width = useContext(WidthContext);
	return (
		<Box flexDirection="row" width={width}>
			<Box flexShrink={0} width={5}>
				<Text color={theme.secondary}>{'  ⎿  '}</Text>
			</Box>
			<Box width={Math.max(10, width - 5)} flexDirection="column">
				{typeof children === 'string' ? (
					<Text color={color} dimColor={dim}>
						{children}
					</Text>
				) : (
					children
				)}
			</Box>
		</Box>
	);
}

function Bullet({color, children, symbol = '⏺'}: {color?: string; children: React.ReactNode; symbol?: string}) {
	const width = useContext(WidthContext);
	return (
		<Box flexDirection="row" width={width}>
			<Box flexShrink={0} width={2}>
				<Text color={color}>{symbol}</Text>
			</Box>
			<Box width={Math.max(10, width - 2)} flexDirection="column">
				{children}
			</Box>
		</Box>
	);
}

function lastLines(text: string, max: number | undefined, width: number): string {
	if (!max) return text;
	const lines = text.split('\n');
	let rows = 0;
	let start = lines.length;
	while (start > 0) {
		const w = stringWidth(lines[start - 1]);
		const h = Math.max(1, Math.ceil(w / Math.max(10, width)));
		if (rows + h > max) break;
		rows += h;
		start--;
	}
	return start > 0 ? lines.slice(start).join('\n') : text;
}

export function ItemView({item, options}: {item: Item; options: ItemOptions}) {
	switch (item.kind) {
		case 'user':
			return <UserPrompt text={item.text} attachments={item.attachments} />;
		case 'text': {
			const rendered = lastLines(renderMarkdown(item.text), options.maxLines, options.columns - 2);
			return (
				<Box marginTop={1}>
					<Bullet color={theme.text} symbol={item.first ? '⏺' : ' '}>
						<Text>{rendered}</Text>
					</Bullet>
				</Box>
			);
		}
		case 'error':
			return (
				<Box marginTop={1} flexDirection="column">
					<Result color={theme.error}>{item.text.trim()}</Result>
				</Box>
			);
		case 'thinking':
			return (
				<Box marginTop={1} flexDirection="column">
					<Text color={theme.secondary} italic>
						{'✻ Thinking…'}
					</Text>
					{options.verbose && (
						<Box paddingLeft={2}>
							<Text color={theme.secondary} italic>
								{lastLines(item.text.trim(), options.maxLines, options.columns - 2)}
							</Text>
						</Box>
					)}
				</Box>
			);
		case 'tool':
			return <ToolCall block={item.block} running={item.running} options={options} />;
		case 'worked':
			return (
				<Box marginTop={1}>
					<Text color={theme.secondary}>{`✻ ${pastVerb(item.key)} for ${formatDuration(item.secs)}`}</Text>
				</Box>
			);
		case 'notice':
			return <NoticeView notice={item.notice} />;
	}
}

const PAST_VERBS = ['Worked', 'Baked', 'Brewed', 'Churned', 'Cooked', 'Crunched', 'Sautéed', 'Simmered', 'Cogitated', 'Oxidized'];

function pastVerb(key: string): string {
	let hash = 0;
	for (const ch of key) hash = (hash * 31 + ch.charCodeAt(0)) | 0;
	return PAST_VERBS[Math.abs(hash) % PAST_VERBS.length];
}

export function UserPrompt({text, attachments = [], bash}: {text: string; attachments?: string[]; bash?: boolean}) {
	const width = useContext(WidthContext);
	return (
		<Box marginTop={1} flexDirection="column">
			<Box flexDirection="row" width={width}>
				<Box flexShrink={0} width={2}>
					<Text color={bash ? theme.bashBorder : theme.secondary}>{bash ? '!' : '>'}</Text>
				</Box>
				<Box width={Math.max(10, width - 2)}>
					<Text color={bash ? undefined : theme.secondary}>{text.trim() || ' '}</Text>
				</Box>
			</Box>
			{attachments.map((name, i) => (
				<Result key={i} color={theme.secondary}>
					{name.startsWith('image') ? `[Image #${i + 1}]` : `Attached ${name}`}
				</Result>
			))}
		</Box>
	);
}

function NoticeView({notice}: {notice: Notice}) {
	switch (notice.type) {
		case 'info':
			return (
				<Box marginTop={1} flexDirection="column">
					<Result color={notice.dim === false ? undefined : theme.secondary}>{notice.text}</Result>
				</Box>
			);
		case 'error':
			return (
				<Box marginTop={1} flexDirection="column">
					<Result color={theme.error}>{notice.text}</Result>
				</Box>
			);
		case 'interrupted':
			return (
				<Box flexDirection="column">
					<Result>
						<Text>
							<Text color={theme.error}>Interrupted</Text>
							<Text color={theme.secondary}> · What should oxi do instead?</Text>
						</Text>
					</Result>
				</Box>
			);
		case 'command':
			return (
				<Box flexDirection="column">
					<UserPrompt text={`/${notice.name}${notice.args ? ' ' + notice.args : ''}`} />
					{notice.output !== undefined && (
						<Result color={notice.error ? theme.error : theme.secondary}>{notice.output}</Result>
					)}
				</Box>
			);
		case 'bash': {
			const out = notice.stdout.replace(/\s+$/, '');
			const err = notice.stderr.replace(/\s+$/, '');
			return (
				<Box flexDirection="column">
					<UserPrompt text={notice.command} bash />
					<Result>
						<Box flexDirection="column">
							{out ? <Text>{out}</Text> : null}
							{err ? <Text color={theme.error}>{err}</Text> : null}
							{!out && !err ? <Text color={theme.secondary}>(No content)</Text> : null}
						</Box>
					</Result>
				</Box>
			);
		}
		case 'lines':
			return (
				<Box marginTop={1} flexDirection="column">
					{notice.lines.map((line, i) => (
						<Text key={i}>{line || ' '}</Text>
					))}
				</Box>
			);
	}
}

const MAX_OUTPUT_LINES = 3;

function ToolCall({block, running, options}: {block: ToolBlock; running: boolean; options: ItemOptions}) {
	const view = toolView(block, options.root);
	const failed = block.status === 'failed';
	const interrupted = block.status === 'interrupted';
	const waiting = block.status === 'pending';
	let dot: string | undefined;
	let symbol = '⏺';
	if (failed || interrupted) dot = theme.error;
	else if (running || waiting) {
		dot = theme.secondary;
		if (!options.blink) symbol = ' ';
	} else dot = theme.success;

	return (
		<Box marginTop={1} flexDirection="column">
			<Bullet color={dot} symbol={symbol}>
				<Text>
					<Text bold>{view.name}</Text>
					{view.args ? <Text>{`(${view.args})`}</Text> : null}
				</Text>
			</Bullet>
			<ToolBody block={block} running={running} options={options} />
		</Box>
	);
}

function ToolBody({block, running, options}: {block: ToolBlock; running: boolean; options: ItemOptions}) {
	const view = toolView(block, options.root);
	const output = (block.output ?? '').replace(/\s+$/, '');

	const todos = todoItems(block);
	if (todos) {
		return (
			<Result>
				<Box flexDirection="column">
					{todos.map((todo, i) => (
						<TodoLine key={i} status={todo.status} content={todo.content} />
					))}
				</Box>
			</Result>
		);
	}

	if (block.status === 'interrupted') return <Result color={theme.error}>Interrupted by user</Result>;
	if (block.status === 'failed') {
		const text = output || 'Error';
		const lines = text.split('\n');
		const shown = options.verbose ? lines : lines.slice(0, 6);
		return (
			<Result>
				<Box flexDirection="column">
					<Text color={theme.error}>{(/^error/i.test(shown[0]) ? '' : 'Error: ') + shown.join('\n')}</Text>
					{lines.length > shown.length && <MoreLines n={lines.length - shown.length} />}
				</Box>
			</Result>
		);
	}
	if (running || block.status === 'pending' || block.status === 'in_progress') {
		if (view.family === 'bash' && output) return <OutputLines text={output} verbose={options.verbose} tail />;
		return view.family === 'bash' ? <Result color={theme.secondary}>Running…</Result> : null;
	}

	if ((view.family === 'edit' || view.family === 'write') && block.diff) {
		const summary = resultSummary(block, view, options.root);
		return (
			<Result>
				<Box flexDirection="column">
					<Text>{summary}</Text>
					<DiffView diff={block.diff} offset={diffOffset(block)} file={view.args} columns={options.columns - 5} max={options.verbose ? undefined : view.family === 'write' ? 10 : 50} />
				</Box>
			</Result>
		);
	}
	const summary = resultSummary(block, view, options.root);
	if (summary && !options.verbose) {
		return (
			<Result>
				<Text>
					<Text>{summary}</Text>
					{view.family === 'read' || view.family === 'search' || view.family === 'list' ? (
						<Text color={theme.secondary}>{output ? ' (ctrl+r to expand)' : ''}</Text>
					) : null}
				</Text>
			</Result>
		);
	}
	if (!output) return view.family === 'bash' ? <Result color={theme.secondary}>(No content)</Result> : null;
	return <OutputLines text={output} verbose={options.verbose} />;
}

function MoreLines({n}: {n: number}) {
	return <Text color={theme.secondary}>{`… +${plural(n, 'line')} (ctrl+r to expand)`}</Text>;
}

function OutputLines({text, verbose, tail}: {text: string; verbose: boolean; tail?: boolean}) {
	const lines = text.split('\n');
	const shown = verbose ? lines : tail ? lines.slice(-MAX_OUTPUT_LINES) : lines.slice(0, MAX_OUTPUT_LINES);
	const hidden = lines.length - shown.length;
	return (
		<Result>
			<Box flexDirection="column">
				{tail && hidden > 0 && <MoreLines n={hidden} />}
				<Text>{shown.join('\n')}</Text>
				{!tail && hidden > 0 && <MoreLines n={hidden} />}
			</Box>
		</Result>
	);
}

export function TodoLine({status, content}: {status: string; content: string}) {
	if (status === 'completed') {
		return (
			<Text color={theme.secondary}>
				{'☒ '}
				<Text strikethrough>{content}</Text>
			</Text>
		);
	}
	if (status === 'in_progress') return <Text bold>{`☐ ${content}`}</Text>;
	return <Text>{`☐ ${content}`}</Text>;
}

/** A diff as Claude Code shows it: line numbers, red and green rows across the width. */
export function DiffView({diff, file, columns, max, offset}: {diff: string; file?: string; columns: number; max?: number; offset?: number}) {
	const {lines} = parseDiff(diff, offset);
	const shown = max ? lines.slice(0, max) : lines;
	const numberWidth = Math.max(3, ...shown.map(line => String(line.line ?? '').length));
	const lang = file ? languageForPath(file) : undefined;
	const width = Math.max(20, columns - 1);
	return (
		<Box flexDirection="column">
			{shown.map((line, i) => (
				<Text key={i}>{diffRow(line, numberWidth, width, lang)}</Text>
			))}
			{lines.length > shown.length && <MoreLines n={lines.length - shown.length} />}
		</Box>
	);
}

function diffRow(line: DiffLine, numberWidth: number, width: number, lang?: string): string {
	if (line.type === 'gap') return chalk.rgb(153, 153, 153)(' '.repeat(numberWidth) + '   ...');
	const sign = line.type === 'add' ? '+' : line.type === 'remove' ? '-' : ' ';
	const number = String(line.line ?? '').padStart(numberWidth);
	const code = lang ? highlight(line.text.replace(/\t/g, '    '), lang) : line.text.replace(/\t/g, '    ');
	const body = `${number} ${sign}  ${code}`;
	const pad = Math.max(0, width - stringWidth(body));
	const row = body + ' '.repeat(pad);
	if (line.type === 'add') return chalk.bgRgb(34, 92, 43)(row);
	if (line.type === 'remove') return chalk.bgRgb(122, 41, 54)(row);
	return chalk.rgb(153, 153, 153)(number) + ` ${sign}  ` + code;
}

/** Preview of the change an approval is about, from the pending call's arguments. */
export function ProposedChange({block, columns, root}: {block: ToolBlock; columns: number; root: string}) {
	const args = parseArgs(block) ?? {};
	const view = toolView(block, root);
	if (block.diff) return <DiffView diff={block.diff} offset={diffOffset(block)} file={view.args} columns={columns} max={30} />;
	if (typeof args.old_string === 'string' || typeof args.new_string === 'string') {
		const start = block.locations?.find(l => l.line)?.line ?? 1;
		const diff = snippetDiff(String(args.old_string ?? ''), String(args.new_string ?? ''), start);
		return <DiffView diff={diff} file={view.args} columns={columns} max={30} />;
	}
	if (typeof args.content === 'string') {
		const lines = args.content.split('\n');
		const lang = languageForPath(view.args);
		const shown = lines.slice(0, 20);
		return (
			<Box flexDirection="column">
				{shown.map((line: string, i: number) => (
					<Text key={i}>
						<Text color={theme.secondary}>{String(i + 1).padStart(3)} </Text>
						{lang ? highlight(line, lang) : line}
					</Text>
				))}
				{lines.length > shown.length && <Text color={theme.secondary}>{`… +${plural(lines.length - shown.length, 'line')}`}</Text>}
			</Box>
		);
	}
	return null;
}
