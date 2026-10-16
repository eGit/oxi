// Project files for `@` mentions: git's list when there is one, else a bounded walk.

import {execFile} from 'node:child_process';
import {readdir} from 'node:fs/promises';
import path from 'node:path';

const SKIP = new Set(['.git', 'node_modules', 'target', 'dist', 'build', '.venv', 'venv', '__pycache__', '.next', '.cache']);
const LIMIT = 50_000;

export async function listFiles(root: string): Promise<string[]> {
	const fromGit = await new Promise<string[] | null>(resolve => {
		execFile(
			'git',
			['ls-files', '--cached', '--others', '--exclude-standard'],
			{cwd: root, maxBuffer: 64 * 1024 * 1024},
			(error, stdout) => resolve(error ? null : stdout.split('\n').filter(Boolean)),
		);
	});
	const files = fromGit ?? (await walk(root));
	const dirs = new Set<string>();
	for (const file of files.slice(0, LIMIT)) {
		let dir = path.posix.dirname(file);
		while (dir && dir !== '.' && !dirs.has(dir + '/')) {
			dirs.add(dir + '/');
			dir = path.posix.dirname(dir);
		}
	}
	return [...dirs, ...files.slice(0, LIMIT)];
}

async function walk(root: string): Promise<string[]> {
	const out: string[] = [];
	const queue = [''];
	while (queue.length && out.length < 5000) {
		const dir = queue.shift()!;
		let entries;
		try {
			entries = await readdir(path.join(root, dir), {withFileTypes: true});
		} catch {
			continue;
		}
		for (const entry of entries) {
			if (entry.name.startsWith('.') && entry.name !== '.github') continue;
			const rel = dir ? `${dir}/${entry.name}` : entry.name;
			if (entry.isDirectory()) {
				if (!SKIP.has(entry.name)) queue.push(rel);
			} else out.push(rel);
		}
	}
	return out;
}
