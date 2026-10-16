import chalk from 'chalk';
import cliTruncate from 'cli-truncate';
import {Box, Text} from 'ink';
import {homedir, userInfo} from 'node:os';
import React from 'react';
import stringWidth from 'string-width';
import type {ChatSummary} from '../model.js';
import {c, fg, theme} from '../theme.js';
import {formatAgeShort} from './format.js';

type Props = {
	columns: number;
	version: string;
	cwd: string;
	model: string;
	chats: ChatSummary[];
};

const MASCOT = [' ▐▛███▜▌ ', '▝▜█████▛▘', '  ▘▘ ▝▝  '];

function fit(text: string, width: number): string {
	const cut = stringWidth(text) > width ? cliTruncate(text, width) : text;
	return cut + ' '.repeat(Math.max(0, width - stringWidth(cut)));
}

function center(text: string, width: number): string {
	const w = stringWidth(text);
	if (w >= width) return fit(text, width);
	const left = Math.floor((width - w) / 2);
	return ' '.repeat(left) + text + ' '.repeat(width - w - left);
}

const tilde = (p: string) => (p.startsWith(homedir()) ? '~' + p.slice(homedir().length) : p);

/** A path cut from the left, so the folder's own name stays. */
function tail(p: string, width: number): string {
	if (stringWidth(p) <= width) return p;
	const parts = p.split('/');
	let out = parts.pop() ?? '';
	while (parts.length && stringWidth('…/' + parts[parts.length - 1] + '/' + out) <= width) out = parts.pop() + '/' + out;
	return stringWidth('…/' + out) <= width ? '…/' + out : '…' + out.slice(-(width - 1));
}

export function Welcome({columns, version, cwd, model, chats}: Props) {
	const border = fg(theme.claude);
	const width = Math.max(40, Math.min(columns, 140));
	const inner = width - 2;
	const name = userInfo().username.replace(/^./, ch => ch.toUpperCase());
	const title = ` oxi v${version} `;
	const top = border('╭───') + chalk.bold(c.claude(title)) + border('─'.repeat(Math.max(0, inner - 3 - stringWidth(title))) + '╮');
	const bottom = border('╰' + '─'.repeat(inner) + '╯');

	const twoColumns = width >= 80;
	const leftWidth = twoColumns ? Math.max(32, Math.floor(inner * 0.42)) : inner;
	const rightWidth = inner - leftWidth - 3;

	const left = [
		'',
		center(chalk.bold(`Welcome back ${name}!`), leftWidth),
		'',
		...MASCOT.map(line => center(c.claude(line), leftWidth)),
		'',
		center(c.secondary(model), leftWidth),
		center(c.secondary(tail(tilde(cwd), leftWidth - 2)), leftWidth),
	];

	const right: string[] = [];
	if (twoColumns) {
		right.push(chalk.bold(c.claude('Tips for getting started')));
		right.push('Run /resume to continue an earlier chat');
		right.push('Use @ to mention files, ! to run a shell command');
		right.push(c.claude('─'.repeat(rightWidth - 1)));
		right.push(chalk.bold(c.claude('Recent activity')));
		if (!chats.length) right.push(c.secondary('No recent activity'));
		for (const chat of chats.slice(0, 3)) {
			right.push(`${c.secondary(formatAgeShort(chat.modified).padStart(3))}  ${chat.title || 'New chat'}`);
		}
		if (chats.length) right.push(c.secondary('/resume for more'));
	}

	const rows = Math.max(left.length, right.length);
	const lines = [top];
	for (let i = 0; i < rows; i++) {
		const l = fit(left[i] ?? '', leftWidth);
		if (twoColumns) {
			const r = fit(right[i] ?? '', rightWidth);
			lines.push(border('│') + l + ' ' + border('│') + ' ' + r + border('│'));
		} else {
			lines.push(border('│') + l + border('│'));
		}
	}
	lines.push(bottom);

	return (
		<Box flexDirection="column">
			<Text>{lines.join('\n')}</Text>
		</Box>
	);
}
