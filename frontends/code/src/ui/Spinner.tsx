import {Box, Text} from 'ink';
import React, {useEffect, useMemo, useState} from 'react';
import {theme} from '../theme.js';
import {formatDuration, formatTokens} from './format.js';
import {TodoLine, Result} from './items.js';

const FRAMES = process.platform === 'darwin' ? ['·', '✢', '✳', '✶', '✻', '✽'] : ['·', '✢', '*', '✶', '✻', '✽'];
const GLYPHS = [...FRAMES, ...[...FRAMES].reverse()];

const VERBS = [
	'Accomplishing', 'Actioning', 'Actualizing', 'Baking', 'Brewing', 'Calculating', 'Cerebrating', 'Churning',
	'Coalescing', 'Cogitating', 'Computing', 'Conjuring', 'Considering', 'Cooking', 'Crafting', 'Creating',
	'Crunching', 'Deliberating', 'Determining', 'Doing', 'Effecting', 'Finagling', 'Forging', 'Forming',
	'Generating', 'Hatching', 'Herding', 'Honking', 'Hustling', 'Ideating', 'Inferring', 'Manifesting',
	'Marinating', 'Moseying', 'Mulling', 'Mustering', 'Musing', 'Noodling', 'Oxidizing', 'Percolating',
	'Pondering', 'Processing', 'Puttering', 'Reticulating', 'Ruminating', 'Schlepping', 'Shucking',
	'Simmering', 'Smooshing', 'Spinning', 'Stewing', 'Synthesizing', 'Thinking', 'Transmuting', 'Vibing',
	'Working',
];

type Props = {
	startedAt: number;
	tokens: number;
	status?: string | null;
	todos?: Array<{status: string; content: string}> | null;
	showTodos: boolean;
	thinking?: boolean;
};

export function Spinner({startedAt, tokens, status, todos, showTodos, thinking}: Props) {
	const [tick, setTick] = useState(0);
	const verb = useMemo(() => VERBS[Math.floor(Math.random() * VERBS.length)], [startedAt]);
	useEffect(() => {
		const timer = setInterval(() => setTick(t => t + 1), 120);
		return () => clearInterval(timer);
	}, []);
	const elapsed = (Date.now() - startedAt) / 1000;
	const glyph = GLYPHS[tick % GLYPHS.length];
	const label = `${verb}…`;
	// A brighter band sweeps across the verb, like Claude Code's shimmer.
	const sweep = (tick % (label.length + 12)) - 4;
	const current = todos?.find(t => t.status === 'in_progress');

	const parts: string[] = [];
	if (elapsed >= 1) parts.push(formatDuration(elapsed));
	if (tokens >= 10) parts.push(`↓ ${formatTokens(tokens)} tokens`);
	if (thinking) parts.push('thinking');
	parts.push('esc to interrupt');
	if (todos?.length) parts.push(`ctrl+t to ${showTodos ? 'hide' : 'show'} todos`);

	return (
		<Box flexDirection="column" marginTop={1}>
			<Box flexDirection="row">
				<Box width={2} flexShrink={0}>
					<Text color={theme.claude}>{glyph}</Text>
				</Box>
				<Text>
					{current ? (
						<Text color={theme.claude}>{current.content}…</Text>
					) : (
						[...label].map((ch, i) => (
							<Text key={i} color={Math.abs(i - sweep) <= 1 ? theme.claudeShimmer : theme.claude}>
								{ch}
							</Text>
						))
					)}
					<Text color={theme.secondary}>{` (${parts.join(' · ')})`}</Text>
				</Text>
			</Box>
			{status ? <Result color={theme.warning}>{status}</Result> : null}
			{todos?.length && showTodos ? (
				<Result>
					<Box flexDirection="column">
						{todos.map((todo, i) => (
							<TodoLine key={i} status={todo.status} content={todo.content} />
						))}
					</Box>
				</Result>
			) : null}
		</Box>
	);
}
