// Prompt history per project, kept in ~/.config/oxi-code/history.jsonl like Claude Code's.

import {appendFileSync, mkdirSync, readFileSync} from 'node:fs';
import {homedir} from 'node:os';
import path from 'node:path';

const FILE = path.join(process.env.XDG_CONFIG_HOME || path.join(homedir(), '.config'), 'oxi-code', 'history.jsonl');

export function loadHistory(root: string): string[] {
	try {
		return readFileSync(FILE, 'utf8')
			.split('\n')
			.filter(Boolean)
			.map(line => {
				try {
					return JSON.parse(line) as {root: string; text: string};
				} catch {
					return null;
				}
			})
			.filter((entry): entry is {root: string; text: string} => entry?.root === root)
			.map(entry => entry.text)
			.slice(-200);
	} catch {
		return [];
	}
}

export function saveHistory(root: string, text: string) {
	try {
		mkdirSync(path.dirname(FILE), {recursive: true});
		appendFileSync(FILE, JSON.stringify({root, text}) + '\n');
	} catch {
		// History is a convenience; a read-only home must not break the prompt.
	}
}
