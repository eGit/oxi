import {realpathSync} from 'node:fs';
import {render} from 'ink';
import React from 'react';
import {App, CLIENT_VERSION} from './app.js';
import {findOxi, OxiClient} from './client.js';

const USAGE = `Usage: oxi-code [options] [prompt]

oxi-code is a Claude Code-style terminal interface for oxi. It talks to the oxi daemon
(\`oxi headless\`), so chats are the same ones the oxi app and the editor plugins show.

Arguments:
  prompt                 Your prompt, sent as the first message

Options:
  -c, --continue         Continue the most recent conversation in this folder
  -r, --resume [id]      Resume a conversation (pick one, or give its id)
  --oxi <path>           The oxi executable (default: $OXI_PATH, PATH, ~/.local/bin/oxi)
  -v, --version          Show the version
  -h, --help             Show this help
`;

function parseArgs(argv: string[]) {
	const out: {prompt: string[]; resume?: string; oxi?: string; help?: boolean; version?: boolean} = {prompt: []};
	for (let i = 0; i < argv.length; i++) {
		const arg = argv[i];
		if (arg === '-c' || arg === '--continue') out.resume = 'continue';
		else if (arg === '-r' || arg === '--resume') {
			const next = argv[i + 1];
			if (next && !next.startsWith('-') && next.includes('/')) {
				out.resume = next;
				i++;
			} else out.resume = 'pick';
		} else if (arg === '--oxi') out.oxi = argv[++i];
		else if (arg === '-h' || arg === '--help') out.help = true;
		else if (arg === '-v' || arg === '--version') out.version = true;
		else out.prompt.push(arg);
	}
	return out;
}

async function main() {
	const args = parseArgs(process.argv.slice(2));
	if (args.help) {
		process.stdout.write(USAGE);
		return;
	}
	if (args.version) {
		process.stdout.write(`${CLIENT_VERSION} (oxi-code)\n`);
		return;
	}
	if (!process.stdin.isTTY || !process.stdout.isTTY) {
		process.stderr.write('oxi-code needs an interactive terminal.\n');
		process.exitCode = 1;
		return;
	}
	const binary = findOxi(args.oxi);
	if (!binary) {
		process.stderr.write('oxi-code: could not find the oxi executable. Install oxi or pass --oxi <path>.\n');
		process.exitCode = 1;
		return;
	}
	let root = process.cwd();
	try {
		root = realpathSync(root);
	} catch {
		// Keep the path as given.
	}

	const client = new OxiClient(binary);
	// Bracketed paste lets the prompt tell a pasted Enter from a typed one.
	process.stdout.write('\u001b[?2004h');
	const restore = () => process.stdout.write('\u001b[?2004l');
	process.on('exit', restore);

	let clear = () => {};
	const instance = render(
		<App
			client={client}
			root={root}
			clearScreen={() => clear()}
			initialPrompt={args.prompt.join(' ').trim() || undefined}
			resume={args.resume}
		/>,
		// Kitty's keyboard protocol (kitty, WezTerm, Ghostty) tells Shift+Enter from Enter.
		{exitOnCtrlC: false, maxFps: 30, kittyKeyboard: {mode: 'auto'}},
	);
	clear = () => {
		instance.clear();
		process.stdout.write('\u001b[2J\u001b[3J\u001b[H');
	};
	await instance.waitUntilExit();
	await client.close();
	restore();
	process.exit(0);
}

main().catch(error => {
	process.stderr.write(`oxi-code: ${error?.stack ?? error}\n`);
	process.exit(1);
});
