// Talks to `oxi headless` over stdio: one JSON object per line each way. `oxi headless` relays
// to the per-user oxi daemon, so notifications may be about chats other editors run.

import {spawn, type ChildProcessWithoutNullStreams} from 'node:child_process';
import {EventEmitter} from 'node:events';
import {existsSync} from 'node:fs';
import {homedir} from 'node:os';
import path from 'node:path';

type Pending = {resolve: (value: any) => void; reject: (error: Error) => void};

export class OxiClient extends EventEmitter {
	private proc: ChildProcessWithoutNullStreams;
	private nextId = 1;
	private pending = new Map<number, Pending>();
	private buffer = '';
	readonly log: string[] = [];
	exited = false;

	constructor(readonly binary: string) {
		super();
		// Detached = its own session, without this terminal: a shell oxi runs (`$SHELL -ilc` to
		// find an agent's PATH) could otherwise take the terminal and suspend oxi-code.
		this.proc = spawn(binary, ['headless'], {cwd: homedir(), stdio: ['pipe', 'pipe', 'pipe'], detached: true});
		this.proc.stdout.setEncoding('utf8');
		this.proc.stdout.on('data', (chunk: string) => this.onData(chunk));
		this.proc.stderr.setEncoding('utf8');
		this.proc.stderr.on('data', (chunk: string) => {
			for (const line of chunk.split('\n')) {
				if (line.trim()) this.log.push(line);
			}
			if (this.log.length > 500) this.log.splice(0, this.log.length - 500);
		});
		this.proc.on('error', error => this.onExit(error.message));
		this.proc.on('exit', code => this.onExit(`oxi exited (${code ?? 'signal'})`));
		this.proc.stdin.on('error', () => {});
	}

	request<T = any>(method: string, params: Record<string, unknown> = {}): Promise<T> {
		if (this.exited) return Promise.reject(new Error('oxi is not running'));
		const id = this.nextId++;
		return new Promise<T>((resolve, reject) => {
			this.pending.set(id, {resolve, reject});
			this.proc.stdin.write(JSON.stringify({id, method, params}) + '\n');
		});
	}

	/** Detach from the daemon; turns keep running there. */
	async close() {
		if (this.exited) return;
		const timeout = new Promise(resolve => setTimeout(resolve, 800));
		await Promise.race([this.request('shutdown').catch(() => {}), timeout]);
		this.proc.kill();
	}

	private onData(chunk: string) {
		this.buffer += chunk;
		let newline: number;
		while ((newline = this.buffer.indexOf('\n')) >= 0) {
			const line = this.buffer.slice(0, newline);
			this.buffer = this.buffer.slice(newline + 1);
			if (!line.trim()) continue;
			let message: any;
			try {
				message = JSON.parse(line);
			} catch {
				this.log.push(`unreadable line: ${line.slice(0, 200)}`);
				continue;
			}
			if (message.method) {
				this.emit('notification', message.method, message.params ?? {});
				continue;
			}
			const pending = this.pending.get(message.id);
			if (!pending) continue;
			this.pending.delete(message.id);
			if (message.error != null) pending.reject(new Error(String(message.error)));
			else pending.resolve(message.result);
		}
	}

	private onExit(reason: string) {
		if (this.exited) return;
		this.exited = true;
		for (const pending of this.pending.values()) pending.reject(new Error(reason));
		this.pending.clear();
		this.emit('exit', reason);
	}
}

/** The oxi executable: `--oxi`, `$OXI_PATH`, then PATH and the usual install locations. */
export function findOxi(explicit?: string): string | undefined {
	const candidates: string[] = [];
	if (explicit) candidates.push(explicit);
	if (process.env.OXI_PATH) candidates.push(process.env.OXI_PATH);
	for (const dir of (process.env.PATH ?? '').split(path.delimiter)) {
		if (dir) candidates.push(path.join(dir, process.platform === 'win32' ? 'oxi.exe' : 'oxi'));
	}
	candidates.push(
		path.join(homedir(), '.local', 'bin', 'oxi'),
		'/Applications/oxi.app/Contents/MacOS/oxi',
		path.join(homedir(), 'Applications', 'oxi.app', 'Contents', 'MacOS', 'oxi'),
	);
	return candidates.find(candidate => existsSync(candidate));
}
