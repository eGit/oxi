import {Box, Text, useInput} from 'ink';
import React, {useState} from 'react';
import {theme} from '../theme.js';

export type Option = {label: string; description?: string; hint?: string; current?: boolean; disabled?: boolean};

type Props = {
	options: Option[];
	initial?: number;
	onSelect: (index: number) => void;
	onCancel?: () => void;
	/** Rows shown at once; the list scrolls past that. */
	visible?: number;
	numbered?: boolean;
	isActive?: boolean;
};

/** A `❯ 1. Option` list: arrows or numbers to choose, Enter to confirm, Esc to leave. */
export function Select({options, initial = 0, onSelect, onCancel, visible = 10, numbered = true, isActive = true}: Props) {
	const [index, setIndex] = useState(Math.min(Math.max(0, initial), Math.max(0, options.length - 1)));
	useInput(
		(input, key) => {
			if (key.upArrow || (key.ctrl && input === 'p')) setIndex(i => (i - 1 + options.length) % options.length);
			else if (key.downArrow || (key.ctrl && input === 'n') || (key.tab && !key.shift)) setIndex(i => (i + 1) % options.length);
			else if (key.return) {
				if (!options[index]?.disabled) onSelect(index);
			} else if (key.escape) onCancel?.();
			else if (numbered && /^[1-9]$/.test(input)) {
				const n = Number(input) - 1;
				if (n < options.length && !options[n].disabled) onSelect(n);
			}
		},
		{isActive},
	);
	const start = Math.max(0, Math.min(index - Math.floor(visible / 2), options.length - visible));
	const shown = options.slice(start, start + visible);
	const labelWidth = Math.max(...shown.map((o, i) => (numbered ? `${start + i + 1}. `.length : 0) + o.label.length));
	return (
		<Box flexDirection="column">
			{start > 0 && <Text color={theme.secondary}>{'  ↑'}</Text>}
			{shown.map((option, i) => {
				const n = start + i;
				const selected = n === index;
				const label = (numbered ? `${n + 1}. ` : '') + option.label;
				return (
					<Box key={n} flexDirection="row">
						<Box width={2} flexShrink={0}>
							<Text color={theme.suggestion}>{selected ? '❯' : ' '}</Text>
						</Box>
						<Text wrap="truncate-end">
							<Text color={option.disabled ? theme.secondary : selected ? theme.suggestion : undefined}>
								{option.description && option.description !== option.label ? label.padEnd(labelWidth + 2) : label}
							</Text>
							{option.description && option.description !== option.label ? <Text color={theme.secondary}>{option.description}</Text> : null}
							{option.current ? <Text color={theme.success}>{' ✔'}</Text> : null}
							{option.hint ? <Text color={theme.secondary}>{option.hint.startsWith(' ') ? option.hint : '  ' + option.hint}</Text> : null}
						</Text>
					</Box>
				);
			})}
			{start + visible < options.length && <Text color={theme.secondary}>{'  ↓'}</Text>}
		</Box>
	);
}
