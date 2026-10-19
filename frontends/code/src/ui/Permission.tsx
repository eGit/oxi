import {Box, Text, useInput} from 'ink';
import path from 'node:path';
import React from 'react';
import type {Approval, ToolBlock} from '../model.js';
import {theme} from '../theme.js';
import {parseArgs, toolView} from '../tools.js';
import {ProposedChange, WidthContext} from './items.js';
import {Select, type Option} from './Select.js';

export type Decision =
	| {decision: 'approve'}
	| {decision: 'approve'; acceptEdits: true}
	| {decision: 'allow_prefix'; prefix: string}
	| {decision: 'allow_tool'}
	| {decision: 'deny'};

type Props = {
	approval: Approval;
	block?: ToolBlock;
	root: string;
	columns: number;
	onDecision: (decision: Decision) => void;
};

export function PermissionPrompt({approval, block, root, columns, onDecision}: Props) {
	const call: ToolBlock = block ?? {type: 'tool', name: approval.tool, status: 'pending'};
	const view = toolView({...call, name: call.name || approval.tool}, root);
	const args = parseArgs(call) ?? {};
	const file = view.args;
	const base = file ? path.basename(file) : 'this file';
	const tilde = (p: string) => (p.startsWith(process.env.HOME ?? '\0') ? '~' + p.slice((process.env.HOME ?? '').length) : p);

	let title = 'Tool use';
	let question = 'Do you want to proceed?';
	let body: React.ReactNode = (
		<Text>
			{view.name}
			{view.args ? `(${view.args})` : ''}
		</Text>
	);
	const choices: Array<{option: Option; decision: Decision}> = [{option: {label: 'Yes'}, decision: {decision: 'approve'}}];

	switch (view.family) {
		case 'bash':
			title = 'Bash command';
			body = (
				<Box flexDirection="column">
					<Text>{String(args.command ?? approval.summary)}</Text>
					{args.description ? <Text color={theme.secondary}>{String(args.description)}</Text> : null}
				</Box>
			);
			if (approval.allow_prefix) {
				choices.push({
					option: {label: `Yes, and don't ask again for ${approval.allow_prefix} commands in ${tilde(root)}`},
					decision: {decision: 'allow_prefix', prefix: approval.allow_prefix},
				});
			}
			break;
		case 'edit':
		case 'write': {
			const creating = view.family === 'write';
			title = creating ? 'Create file' : 'Edit file';
			question = creating ? `Do you want to create ${base}?` : `Do you want to make this edit to ${base}?`;
			body = (
				<Box flexDirection="column">
					<Text bold>{file}</Text>
					<ProposedChange block={call} columns={columns - 8} root={root} />
				</Box>
			);
			choices.push({
				option: {label: 'Yes, allow all edits during this session (shift+tab)'},
				decision: {decision: 'approve', acceptEdits: true},
			});
			break;
		}
		case 'fetch':
			title = 'Fetch';
			question = 'Do you want to allow oxi to fetch this content?';
			body = <Text>{view.args || approval.summary}</Text>;
			choices.push({option: {label: `Yes, and don't ask again for ${view.name} in ${tilde(root)}`}, decision: {decision: 'allow_tool'}});
			break;
		default:
			if (approval.summary && approval.summary !== view.args) {
				body = (
					<Box flexDirection="column">
						{body}
						<Text color={theme.secondary}>{approval.summary}</Text>
					</Box>
				);
			}
			choices.push({option: {label: `Yes, and don't ask again for ${view.name} in ${tilde(root)}`}, decision: {decision: 'allow_tool'}});
	}
	choices.push({option: {label: 'No, and tell oxi what to do differently', hint: ' (esc)'}, decision: {decision: 'deny'}});

	useInput((_input, key) => {
		if (key.tab && key.shift && (view.family === 'edit' || view.family === 'write')) {
			onDecision({decision: 'approve', acceptEdits: true});
		}
	});

	return (
		<Box flexDirection="column" borderStyle="round" borderColor={theme.permission} paddingX={1} marginTop={1}>
			<Text bold color={theme.permission}>
				{title}
			</Text>
			<Box paddingX={2} marginTop={1} marginBottom={1} flexDirection="column">
				<WidthContext.Provider value={Math.max(20, columns - 8)}>{body}</WidthContext.Provider>
			</Box>
			<Text>{question}</Text>
			<Select
				options={choices.map(c => c.option)}
				onSelect={i => onDecision(choices[i].decision)}
				onCancel={() => onDecision({decision: 'deny'})}
			/>
		</Box>
	);
}
