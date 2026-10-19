import {spawn} from 'node:child_process';
import {homedir} from 'node:os';
import path from 'node:path';
import chalk from 'chalk';
import {Box, Static, Text, useApp, useStdout} from 'ink';
import React, {useCallback, useEffect, useMemo, useRef, useState} from 'react';
import type {OxiClient} from './client.js';
import {listFiles} from './files.js';
import {loadHistory, saveHistory} from './history.js';
import {
	flatten,
	streamedChars,
	todoItems,
	type Approval,
	type AssistantMessage,
	type ChatSummary,
	type Config,
	type Item,
	type LocalNotice,
	type Message,
	type Notice,
	type Provider,
	type Quota,
	type ToolBlock,
} from './model.js';
import {c, theme} from './theme.js';
import {formatAge, formatResetsIn, formatTokens} from './ui/format.js';
import {ItemView, WidthContext, type ItemOptions} from './ui/items.js';
import {PermissionPrompt, type Decision} from './ui/Permission.js';
import {PromptInput, type Command, type Image, type Mode, type Submission} from './ui/PromptInput.js';
import {Select, type Option} from './ui/Select.js';
import {Spinner} from './ui/Spinner.js';
import {Welcome} from './ui/Welcome.js';

export const CLIENT_VERSION = '0.1.0';

const COMMANDS: Command[] = [
	{name: 'clear', aliases: ['new', 'reset'], description: 'Clear conversation history and free up context'},
	{name: 'resume', description: 'Resume a conversation'},
	{name: 'model', args: '[model]', description: 'Set the AI model for oxi'},
	{name: 'effort', args: '[level]', description: 'Set the reasoning effort'},
	{name: 'plan', description: 'Toggle plan mode'},
	{name: 'context', description: 'Show current context usage'},
	{name: 'usage', description: 'Show plan usage limits'},
	{name: 'status', description: 'Show oxi status including version, model, and connectivity'},
	{name: 'rename', args: '<title>', description: 'Rename the current conversation'},
	{name: 'help', description: 'Show help and available commands'},
	{name: 'exit', aliases: ['quit'], description: 'Exit the REPL'},
];

const EFFORTS = ['', 'low', 'medium', 'high', 'xhigh', 'max'];
const MODES: Mode[] = ['default', 'acceptEdits', 'plan'];

type Dialog =
	| {kind: 'resume'; chats: ChatSummary[]}
	| {kind: 'provider'}
	| {kind: 'model'; provider: Provider; models: string[]}
	| {kind: 'effort'};

type StaticItem = Item | {kind: 'welcome'; key: string};

type Props = {
	client: OxiClient;
	root: string;
	clearScreen: () => void;
	initialPrompt?: string;
	resume?: 'continue' | 'pick' | string;
};

const isEditTool = (tool: string) => /edit|write|str_replace|notebook/i.test(tool);

function useColumns() {
	const {stdout} = useStdout();
	const [size, setSize] = useState({columns: stdout.columns || 80, rows: stdout.rows || 24});
	useEffect(() => {
		const onResize = () => setSize({columns: stdout.columns || 80, rows: stdout.rows || 24});
		stdout.on('resize', onResize);
		return () => {
			stdout.off('resize', onResize);
		};
	}, [stdout]);
	return size;
}

export function App({client, root, clearScreen, initialPrompt, resume}: Props) {
	const app = useApp();
	const {columns, rows} = useColumns();

	const [ready, setReady] = useState(false);
	const [version, setVersion] = useState('');
	const [providers, setProviders] = useState<Provider[]>([]);
	const [config, setConfig] = useState<Config | null>(null);
	const [recent, setRecent] = useState<ChatSummary[]>([]);

	const [chatId, setChatIdState] = useState<string | null>(null);
	const chatRef = useRef<string | null>(null);
	const setChatId = (id: string | null) => {
		chatRef.current = id;
		setChatIdState(id);
	};
	const [messages, setMessages] = useState<Array<Message | undefined>>([]);
	const [notices, setNotices] = useState<LocalNotice[]>([]);
	const [running, setRunning] = useState(false);
	const [queuedTexts, setQueuedTexts] = useState<string[]>([]);
	const [approval, setApproval] = useState<Approval | null>(null);
	const [status, setStatus] = useState<string | null>(null);
	const [context, setContext] = useState<{used: number; size: number} | null>(null);
	const [quota, setQuota] = useState<Quota | null>(null);
	const [mode, setModeState] = useState<Mode>('default');
	const modeRef = useRef<Mode>('default');
	const setMode = (m: Mode) => {
		modeRef.current = m;
		setModeState(m);
	};
	const [verbose, setVerbose] = useState(false);
	const [showTodos, setShowTodos] = useState(true);
	const [epoch, setEpoch] = useState(0);
	const [dialog, setDialog] = useState<Dialog | null>(null);
	const [footerNotice, setFooterNotice] = useState<string | null>(null);
	const [turnStart, setTurnStart] = useState(Date.now());
	const [history, setHistory] = useState<string[]>(() => loadHistory(root));
	const [files, setFiles] = useState<string[]>([]);
	const [blink, setBlink] = useState(true);
	const bashContext = useRef<Array<{name: string; text: string}>>([]);
	const committed = useRef(0);
	const exitArmed = useRef<{key: string; at: number} | null>(null);
	const noticeCounter = useRef(0);
	const messagesRef = useRef(messages);
	messagesRef.current = messages;

	const addNotice = useCallback((notice: Notice) => {
		const key = `n${++noticeCounter.current}`;
		setNotices(list => [...list, {key, afterMessages: messagesRef.current.length, notice}]);
	}, []);

	/** Start the transcript over: the screen is cleared and every item printed again. */
	const redraw = useCallback(() => {
		clearScreen();
		committed.current = 0;
		setEpoch(e => e + 1);
	}, [clearScreen]);

	// What went to the scrollback keeps the width it was printed at, so a resize prints it all
	// again at the new width, like Claude Code does.
	const firstSize = useRef(true);
	useEffect(() => {
		if (firstSize.current) {
			firstSize.current = false;
			return;
		}
		const timer = setTimeout(redraw, 150);
		return () => clearTimeout(timer);
	}, [columns, redraw]);

	const refreshQuota = useCallback(
		(provider?: string) => {
			if (!provider) return;
			client
				.request<Quota>('quota.get', {provider})
				.then(q => setQuota(q))
				.catch(() => setQuota(null));
		},
		[client],
	);

	const applyOpen = useCallback((open: any) => {
		setMessages(open.messages ?? []);
		setRunning(!!open.running);
		setApproval(open.approval ?? null);
		setStatus(open.status ?? null);
		setContext(open.context ?? null);
		if (open.config) setConfig(open.config);
		if (open.plan) setMode('plan');
		else if (modeRef.current === 'plan') setMode('default');
		if (open.running) setTurnStart(Date.now());
	}, []);

	const openChat = useCallback(
		async (id: string) => {
			const open = await client.request('chat.open', {root, chat: id});
			setChatId(id);
			setNotices([]);
			setQueuedTexts([]);
			applyOpen(open);
			redraw();
			refreshQuota(open.config?.provider);
		},
		[client, root, applyOpen, redraw, refreshQuota],
	);

	const newChat = useCallback(() => {
		setChatId(null);
		setMessages([]);
		setNotices([]);
		setRunning(false);
		setApproval(null);
		setStatus(null);
		setContext(null);
		setQueuedTexts([]);
		if (modeRef.current === 'plan') setMode('default');
		redraw();
	}, [redraw]);

	// Connect: providers, the chat's config, recent chats; then what the command line asked.
	useEffect(() => {
		let cancelled = false;
		(async () => {
			try {
				const init = await client.request('initialize');
				if (cancelled) return;
				setVersion(init.version ?? '');
				setProviders(init.providers ?? []);
				const cfg = await client.request<Config>('chat.set_config', {root});
				setConfig(cfg);
				const chats = await client.request<ChatSummary[]>('chats.list', {root}).catch(() => []);
				const sorted = [...chats].sort((a, b) => b.modified - a.modified);
				setRecent(sorted);
				setReady(true);
				refreshQuota(cfg.provider);
				if (resume === 'continue' && sorted[0]) await openChat(sorted[0].id);
				else if (resume === 'pick') setDialog({kind: 'resume', chats: sorted});
				else if (resume) await openChat(resume);
				if (initialPrompt) submitRef.current?.({text: initialPrompt, images: [], bash: false});
			} catch (error) {
				setReady(true);
				addNotice({type: 'error', text: `Could not connect to oxi: ${(error as Error).message}`});
			}
		})();
		listFiles(root).then(setFiles, () => {});
		return () => {
			cancelled = true;
		};
	}, []);

	// Daemon notifications about the chat on screen.
	useEffect(() => {
		const onNotification = (method: string, params: any) => {
			const ours = params.chat !== undefined && params.chat === chatRef.current;
			switch (method) {
				case 'message':
					if (!ours) return;
					setMessages(list => {
						const next = [...list];
						next[params.index] = params.message;
						return next;
					});
					return;
				case 'turn':
					if (!ours) return;
					setRunning(was => {
						if (params.running && !was) setTurnStart(Date.now());
						return !!params.running;
					});
					setQueuedTexts(list => list.slice(Math.max(0, list.length - (params.queued ?? 0))));
					if (params.outcome === 'cancelled') addNotice({type: 'interrupted'});
					if (!params.running) {
						setStatus(null);
						refreshQuota(configRef.current?.provider);
						client
							.request<ChatSummary[]>('chats.list', {root})
							.then(chats => setRecent([...chats].sort((a, b) => b.modified - a.modified)))
							.catch(() => {});
					}
					return;
				case 'approval':
					if (!ours) return;
					if (params.approval && modeRef.current === 'acceptEdits' && isEditTool(params.approval.tool)) {
						client.request('approval.respond', {chat: params.chat, decision: 'approve'}).catch(() => {});
						return;
					}
					setApproval(params.approval ?? null);
					return;
				case 'status':
					if (ours) setStatus(params.text ?? null);
					return;
				case 'context':
					if (ours) setContext({used: params.used, size: params.size});
					return;
				case 'config':
					if (ours) setConfig(params.config);
					return;
				case 'chat_reloaded':
					if (ours) openChat(params.chat).catch(() => {});
					return;
				case 'chat_removed':
					if (ours) {
						newChat();
						addNotice({type: 'info', text: 'This conversation was deleted.'});
					}
					return;
			}
		};
		const onExit = (reason: string) => {
			addNotice({type: 'error', text: `Lost connection to oxi: ${reason}`});
			setRunning(false);
		};
		client.on('notification', onNotification);
		client.on('exit', onExit);
		return () => {
			client.off('notification', onNotification);
			client.off('exit', onExit);
		};
	}, [client, openChat, newChat, addNotice, refreshQuota]);

	const configRef = useRef(config);
	configRef.current = config;

	// Usage windows move on their own; read them now and then.
	useEffect(() => {
		const timer = setInterval(() => refreshQuota(configRef.current?.provider), 120_000);
		return () => clearInterval(timer);
	}, [refreshQuota]);

	const flat = useMemo(() => flatten(messages, notices), [messages, notices]);
	let finalPrefix = 0;
	while (finalPrefix < flat.length && flat[finalPrefix].final) finalPrefix++;
	committed.current = Math.max(committed.current, finalPrefix);
	const staticItems: StaticItem[] = useMemo(
		() => (ready ? [{kind: 'welcome', key: 'welcome'} as StaticItem, ...flat.slice(0, committed.current).map(f => f.item)] : []),
		[ready, flat, committed.current, epoch],
	);
	const liveItems = flat.slice(committed.current).map(f => f.item);
	const hasRunningTool = liveItems.some(item => item.kind === 'tool' && item.running);

	useEffect(() => {
		if (!hasRunningTool) return;
		const timer = setInterval(() => setBlink(b => !b), 500);
		return () => clearInterval(timer);
	}, [hasRunningTool]);

	const last = [...messages].reverse().find(m => m?.role === 'assistant') as AssistantMessage | undefined;
	const lastTodos = useMemo(() => {
		if (!last) return null;
		const todo = [...last.blocks].reverse().find(b => b.type === 'tool' && todoItems(b)) as ToolBlock | undefined;
		return todo ? todoItems(todo) : null;
	}, [last]);
	const pendingBlock = useMemo(() => {
		if (!last || !approval) return undefined;
		return [...last.blocks].reverse().find(b => b.type === 'tool' && (b.status === 'pending' || b.status === 'in_progress')) as
			| ToolBlock
			| undefined;
	}, [last, approval]);
	const thinking = !!last?.streaming && last.blocks[last.blocks.length - 1]?.type === 'thinking';

	const options: ItemOptions = {root, columns, verbose};
	const liveOptions: ItemOptions = {...options, blink, maxLines: Math.max(6, rows - 14)};

	const flash = (text: string, ms = 1500) => {
		setFooterNotice(text);
		setTimeout(() => setFooterNotice(n => (n === text ? null : n)), ms);
	};

	const exit = useCallback(async () => {
		await client.close();
		app.exit();
	}, [client, app]);

	const runBash = (command: string) => {
		const shell = process.env.SHELL || '/bin/bash';
		const child = spawn(shell, ['-lc', command], {cwd: root, env: process.env});
		let stdout = '';
		let stderr = '';
		child.stdout.on('data', d => (stdout += d));
		child.stderr.on('data', d => (stderr += d));
		const timer = setTimeout(() => child.kill(), 120_000);
		child.on('close', code => {
			clearTimeout(timer);
			const cut = (s: string) => (s.length > 30_000 ? s.slice(-30_000) : s);
			addNotice({type: 'bash', command, stdout: cut(stdout), stderr: cut(stderr), code});
			bashContext.current.push({
				name: `! ${command}`,
				text: `<bash-input>${command}</bash-input>\n<bash-stdout>${cut(stdout)}</bash-stdout>\n<bash-stderr>${cut(stderr)}</bash-stderr>`,
			});
		});
		child.on('error', error => addNotice({type: 'bash', command, stdout: '', stderr: error.message, code: null}));
	};

	const setConfigOn = async (fields: Record<string, string>) => {
		const params: Record<string, unknown> = {root, ...fields};
		if (chatRef.current) params.chat = chatRef.current;
		const cfg = await client.request<Config>('chat.set_config', params);
		setConfig(cfg);
		refreshQuota(cfg.provider);
		return cfg;
	};

	const runCommand = async (name: string, args: string) => {
		const command = COMMANDS.find(c => c.name === name || c.aliases?.includes(name));
		const echo = (output?: string, error?: boolean) => addNotice({type: 'command', name, args, output, error});
		if (!command) {
			echo(`Unknown command: /${name}`, true);
			return;
		}
		try {
			switch (command.name) {
				case 'clear':
					newChat();
					return;
				case 'exit':
					await exit();
					return;
				case 'resume': {
					const chats = await client.request<ChatSummary[]>('chats.list', {root});
					setDialog({kind: 'resume', chats: [...chats].sort((a, b) => b.modified - a.modified)});
					return;
				}
				case 'model': {
					if (args) {
						const cfg = await setConfigOn({model: args});
						echo(`Set model to ${chalk.bold(cfg.model)}`);
						return;
					}
					setDialog({kind: 'provider'});
					return;
				}
				case 'effort': {
					if (args) {
						const cfg = await setConfigOn({effort: args === 'default' ? '' : args});
						echo(`Set effort to ${chalk.bold(cfg.effort || 'default')}`);
						return;
					}
					setDialog({kind: 'effort'});
					return;
				}
				case 'plan': {
					const next = modeRef.current === 'plan' ? 'default' : 'plan';
					setMode(next);
					echo(next === 'plan' ? 'Plan mode on' : 'Plan mode off');
					return;
				}
				case 'rename': {
					if (!args) return echo('Usage: /rename <title>', true);
					if (!chatRef.current) return echo('Nothing to rename yet: send a message first', true);
					await client.request('chat.rename', {root, chat: chatRef.current, title: args});
					echo(`Renamed to ${chalk.bold(args)}`);
					return;
				}
				case 'context': {
					if (!context?.size) return echo('No context usage reported yet for this conversation');
					const pct = Math.min(1, context.used / context.size);
					const width = 40;
					const filled = Math.round(pct * width);
					addNotice({type: 'command', name, args});
					addNotice({
						type: 'lines',
						lines: [
							`  ${chalk.bold('Context Usage')}`,
							`  ${c.claude('⛁ '.repeat(Math.max(0, Math.round(filled / 2))))}${c.secondary('⛶ '.repeat(Math.max(0, width / 2 - Math.round(filled / 2))))}`,
							`  ${config?.model ?? ''} · ${formatTokens(context.used)}/${formatTokens(context.size)} tokens (${Math.round(pct * 100)}%)`,
						],
					});
					return;
				}
				case 'usage': {
					const q = await client.request<Quota>('quota.get', {provider: config?.provider ?? ''});
					setQuota(q);
					addNotice({type: 'command', name, args});
					addNotice({type: 'lines', lines: usageLines(q, config)});
					return;
				}
				case 'status': {
					const hello = await client.request('daemon.hello', {}).catch(() => null);
					const label = (key: string, value: string) => `  ${chalk.bold(key)}: ${value}`;
					addNotice({type: 'command', name, args});
					addNotice({
						type: 'lines',
						lines: [
							label('Version', `oxi ${version} · oxi-code ${CLIENT_VERSION}`),
							label('Session', chatRef.current ? path.basename(chatRef.current) : c.secondary('new conversation')),
							label('cwd', root.startsWith(homedir()) ? '~' + root.slice(homedir().length) : root),
							'',
							label('Provider', `${config?.label ?? '?'} ${c.secondary(`(${config?.provider ?? ''})`)}`),
							label('Model', config?.model || c.secondary('default')),
							label('Effort', config?.effort || c.secondary('default')),
							'',
							label(
								'oxi',
								hello
									? `${hello.daemon ? 'daemon' : 'in-process'} pid ${hello.pid} · ${hello.clients} ${hello.clients === 1 ? 'client' : 'clients'} · ${hello.chats_running} running`
									: c.secondary('unknown'),
							),
							label('Binary', client.binary),
						],
					});
					return;
				}
				case 'help': {
					const width = Math.max(...COMMANDS.map(cmd => cmd.name.length + (cmd.args ? cmd.args.length + 1 : 0))) + 3;
					addNotice({type: 'command', name, args});
					addNotice({
						type: 'lines',
						lines: [
							`  ${chalk.bold(c.claude('oxi-code'))} ${c.secondary(`v${CLIENT_VERSION}`)} · Claude Code-style interface for oxi`,
							'',
							`  ${chalk.bold('Usage modes:')}`,
							`  • REPL: ${chalk.bold('oxi-code')} (interactive session)`,
							`  • Continue: ${chalk.bold('oxi-code -c')} · Resume: ${chalk.bold('oxi-code -r')}`,
							'',
							`  ${chalk.bold('Common tasks:')}`,
							`  • Ask questions about your codebase ${c.secondary('> How does foo.py work?')}`,
							`  • Edit files ${c.secondary('> Update bar.ts to...')}`,
							`  • Run commands ${c.secondary('> ! ls')}`,
							'',
							`  ${chalk.bold('Interactive mode commands:')}`,
							...COMMANDS.map(cmd => `  ${chalk.bold(('/' + cmd.name + (cmd.args ? ' ' + cmd.args : '')).padEnd(width))} - ${cmd.description}`),
							'',
							`  ${chalk.bold('Shortcuts:')} esc interrupt · esc esc clear input · shift+tab cycle modes · ctrl+r verbose · ctrl+t todos · ctrl+v paste image · \\⏎ newline`,
						],
					});
					return;
				}
			}
		} catch (error) {
			echo((error as Error).message, true);
		}
	};

	const submit = async ({text, images, bash}: Submission) => {
		if (bash) {
			runBash(text);
			return;
		}
		const slash = text.match(/^\/([\w-]+)\s*([\s\S]*)$/);
		if (slash && COMMANDS.some(cmd => cmd.name === slash[1] || cmd.aliases?.includes(slash[1]))) {
			await runCommand(slash[1], slash[2].trim());
			return;
		}
		setHistory(h => [...h.filter(x => x !== text), text]);
		saveHistory(root, text);

		const attachments: Array<Record<string, string>> = [...images.map((image: Image) => ({mime: image.mime, data: image.data}))];
		attachments.push(...bashContext.current);
		bashContext.current = [];
		const params: Record<string, unknown> = {root, text, plan: modeRef.current === 'plan', attachments};
		const id = chatRef.current;
		if (id) params.chat = id;
		if (running && id) {
			setQueuedTexts(list => [...list, text]);
		} else if (!id) {
			// Shown right away; the chat's own copy replaces it once oxi made the chat.
			setMessages([{role: 'user', text, attachments: images.map(i => `image (${i.mime})`)}]);
			setRunning(true);
			setTurnStart(Date.now());
		}
		try {
			const result = await client.request('prompt.send', params);
			if (!id) {
				setChatId(result.chat);
				const open = await client.request('chat.open', {root, chat: result.chat});
				applyOpen(open);
			}
		} catch (error) {
			setRunning(false);
			addNotice({type: 'error', text: (error as Error).message});
		}
	};
	const submitRef = useRef<typeof submit | null>(null);
	submitRef.current = submit;

	const respond = (decision: Decision) => {
		const id = chatRef.current;
		if (!id) return;
		const params: Record<string, unknown> = {chat: id, decision: decision.decision};
		if (decision.decision === 'allow_prefix') params.prefix = decision.prefix;
		if ('acceptEdits' in decision) setMode('acceptEdits');
		setApproval(null);
		client.request('approval.respond', params).catch(error => addNotice({type: 'error', text: (error as Error).message}));
	};

	const onEscape = () => {
		if (running && chatRef.current) {
			client.request('cancel', {chat: chatRef.current}).catch(() => {});
			setQueuedTexts([]);
			return true;
		}
		return false;
	};

	const onExitRequest = (key: 'c' | 'd') => {
		const armed = exitArmed.current;
		if (armed && armed.key === key && Date.now() - armed.at < 1500) {
			exit();
			return;
		}
		exitArmed.current = {key, at: Date.now()};
		flash(`Press Ctrl-${key.toUpperCase()} again to exit`);
	};

	const pasteImage = async (): Promise<Image | null> => {
		try {
			const image = await client.request<Image | null>('clipboard.image');
			if (!image) flash('No image found in clipboard');
			return image;
		} catch (error) {
			flash((error as Error).message);
			return null;
		}
	};

	const usedPct = context?.size ? context.used / context.size : 0;
	const footerRight =
		usedPct >= 0.7 ? (
			<Text color={usedPct >= 0.9 ? theme.error : theme.warning}>{`Context left: ${Math.max(0, Math.round((1 - usedPct) * 100))}%`}</Text>
		) : null;
	const statusLine = config ? <StatusLine config={config} quota={quota} /> : null;

	const renderStatic = (item: StaticItem) =>
		item.kind === 'welcome' ? (
			<Box key={`welcome-${epoch}`} flexDirection="column">
				<Welcome
					columns={columns}
					version={version}
					cwd={root}
					model={config ? `${config.label}${config.model ? ' · ' + config.model : ''}` : ''}
					chats={recent}
				/>
			</Box>
		) : (
			<ItemView key={`${epoch}-${item.key}`} item={item} options={options} />
		);

	if (!ready) {
		return <Text color={theme.secondary}>Connecting to oxi…</Text>;
	}

	return (
		<WidthContext.Provider value={columns}>
			<Static key={epoch} items={staticItems}>
				{renderStatic}
			</Static>
			{liveItems.map(item => (
				<ItemView key={item.key} item={item} options={liveOptions} />
			))}
			{running && !approval ? (
				<Spinner
					startedAt={turnStart}
					tokens={streamedChars(last) / 4}
					status={status}
					todos={lastTodos}
					showTodos={showTodos}
					thinking={thinking}
				/>
			) : null}
			{queuedTexts.map((text, i) => (
				<Box key={i} flexDirection="row">
					<Box width={2} flexShrink={0}>
						<Text color={theme.secondary}>{'>'}</Text>
					</Box>
					<Text color={theme.secondary}>{text}</Text>
				</Box>
			))}
			{approval ? (
				<PermissionPrompt approval={approval} block={pendingBlock} root={root} columns={columns} onDecision={respond} />
			) : null}
			{dialog ? (
				<DialogView
					dialog={dialog}
					config={config}
					providers={providers}
					chatId={chatId}
					onClose={() => setDialog(null)}
					onResume={id => {
						setDialog(null);
						openChat(id).catch(error => addNotice({type: 'error', text: (error as Error).message}));
					}}
					onProvider={async provider => {
						setDialog(null);
						if (!provider.acp) {
							const cfg = await setConfigOn({provider: provider.id});
							addNotice({type: 'command', name: 'model', args: '', output: `Set model to ${chalk.bold(cfg.label)}${cfg.model ? ' · ' + cfg.model : ''}`});
							return;
						}
						flash('Loading models…', 30_000);
						const params: Record<string, unknown> = {root, provider: provider.id};
						if (chatRef.current && config?.provider === provider.id) params.chat = chatRef.current;
						try {
							const list = await client.request<{models: string[]; current: string}>('models.list', params);
							setFooterNotice(null);
							if (!list.models.length) {
								const cfg = await setConfigOn({provider: provider.id});
								addNotice({type: 'command', name: 'model', args: '', output: `Set model to ${chalk.bold(cfg.label)}`});
								return;
							}
							setDialog({kind: 'model', provider, models: list.models});
						} catch (error) {
							setFooterNotice(null);
							addNotice({type: 'error', text: (error as Error).message});
						}
					}}
					onModel={async (provider, model) => {
						setDialog(null);
						const cfg = await setConfigOn({provider: provider.id, model});
						addNotice({type: 'command', name: 'model', args: '', output: `Set model to ${chalk.bold(cfg.label)} · ${chalk.bold(cfg.model)}`});
					}}
					onEffort={async effort => {
						setDialog(null);
						const cfg = await setConfigOn({effort});
						addNotice({type: 'command', name: 'effort', args: '', output: `Set effort to ${chalk.bold(cfg.effort || 'default')}`});
					}}
				/>
			) : null}
			<Box display={approval || dialog ? 'none' : 'flex'} flexDirection="column">
				<PromptInput
					active={!approval && !dialog}
					mode={mode}
					history={history}
					commands={COMMANDS}
					files={() => files}
					onSubmit={submission => {
						submit(submission);
					}}
					onCycleMode={() => setMode(MODES[(MODES.indexOf(modeRef.current) + 1) % MODES.length])}
					onEscape={onEscape}
					onExitRequest={onExitRequest}
					onToggleVerbose={() => {
						setVerbose(v => !v);
						redraw();
					}}
					onToggleTodos={() => setShowTodos(s => !s)}
					onPasteImage={pasteImage}
					notice={footerNotice ?? (verbose ? 'Showing detailed transcript · ctrl+r to toggle' : null)}
					footerRight={footerRight}
					statusLine={statusLine}
				/>
			</Box>
		</WidthContext.Provider>
	);
}

function StatusLine({config, quota}: {config: Config; quota: Quota | null}) {
	const windows = quota?.windows?.filter(w => !w.scope) ?? [];
	return (
		<Text color={theme.secondary} wrap="truncate-end">
			{config.label}
			{config.model ? ` · ${config.model}` : ''}
			{config.effort ? ` · ${config.effort}` : ''}
			{windows.map((w, i) => {
				const color = w.used_pct >= 90 ? theme.error : w.used_pct >= 70 ? theme.warning : theme.secondary;
				return (
					<Text key={i}>
						{i === 0 ? ' · usage ' : ' · '}
						<Text color={color}>{`${w.label} ${Math.round(w.used_pct)}%`}</Text>
						{i === 0 && w.resets_in ? ` ↻${formatResetsIn(w.resets_in)}` : ''}
					</Text>
				);
			})}
		</Text>
	);
}

function usageLines(q: Quota, config: Config | null): string[] {
	if (!q.supported) return [`  ${c.secondary(`Usage limits are not available for ${config?.label ?? q.provider}`)}`];
	if (!q.enabled) return [`  ${c.secondary('Reading usage is turned off for this provider in oxi settings')}`];
	if (q.error && !q.windows.length) return [`  ${c.error(q.error)}`];
	const lines: string[] = [];
	if (q.plan) lines.push(`  ${chalk.bold('Plan')}: ${q.plan}`, '');
	for (const w of q.windows) {
		const width = 50;
		const filled = Math.round((Math.min(100, w.used_pct) / 100) * width);
		const color = w.used_pct >= 90 ? c.error : w.used_pct >= 70 ? c.warning : c.suggestion;
		lines.push(`  ${chalk.bold(w.label + (w.scope ? ` (${w.scope})` : ''))}`);
		lines.push(`  ${color('█'.repeat(filled))}${c.secondary('░'.repeat(width - filled))}  ${Math.round(w.used_pct)}% used`);
		lines.push(`  ${c.secondary(w.resets_in ? `Resets in ${formatResetsIn(w.resets_in)}` : '')}`);
		lines.push('');
	}
	if (q.limit_reached) lines.push(`  ${c.error('Limit reached')}`);
	return lines.length ? lines : [`  ${c.secondary('No usage reported yet')}`];
}

type DialogProps = {
	dialog: Dialog;
	config: Config | null;
	providers: Provider[];
	chatId: string | null;
	onClose: () => void;
	onResume: (id: string) => void;
	onProvider: (provider: Provider) => void;
	onModel: (provider: Provider, model: string) => void;
	onEffort: (effort: string) => void;
};

function DialogView({dialog, config, providers, onClose, onResume, onProvider, onModel, onEffort}: DialogProps) {
	let title = '';
	let subtitle = '';
	let options: Option[] = [];
	let initial = 0;
	let onSelect: (i: number) => void = () => {};
	switch (dialog.kind) {
		case 'resume': {
			title = 'Resume a conversation';
			subtitle = dialog.chats.length ? '' : 'No conversations found in this folder';
			options = dialog.chats.map(chat => ({
				label: chat.title || 'New chat',
				description: undefined,
				hint: formatAge(chat.modified) + (chat.running ? ' · running' : '') + (chat.approval ? ' · needs approval' : ''),
			}));
			onSelect = i => onResume(dialog.chats[i].id);
			break;
		}
		case 'provider':
			title = 'Select model';
			subtitle = 'Switch between the agents and providers set up in oxi. Applies to this conversation.';
			options = providers.map(p => ({label: p.label, description: p.description, current: p.id === config?.provider}));
			initial = Math.max(0, providers.findIndex(p => p.id === config?.provider));
			onSelect = i => onProvider(providers[i]);
			break;
		case 'model': {
			title = `Select model · ${dialog.provider.label}`;
			const current = config?.provider === dialog.provider.id ? config.model : dialog.provider.model;
			options = dialog.models.map(m => ({label: m, current: m === current}));
			initial = Math.max(0, dialog.models.indexOf(current));
			onSelect = i => onModel(dialog.provider, dialog.models[i]);
			break;
		}
		case 'effort':
			title = 'Reasoning effort';
			options = EFFORTS.map(e => ({label: e || 'default', current: e === (config?.effort ?? '')}));
			initial = Math.max(0, EFFORTS.indexOf(config?.effort ?? ''));
			onSelect = i => onEffort(EFFORTS[i]);
			break;
	}
	return (
		<Box flexDirection="column" borderStyle="round" borderColor={theme.permission} paddingX={1} marginTop={1}>
			<Text bold color={theme.permission}>
				{title}
			</Text>
			{subtitle ? <Text color={theme.secondary}>{subtitle}</Text> : null}
			<Box marginTop={1} flexDirection="column">
				{options.length ? (
					<Select options={options} initial={initial} onSelect={onSelect} onCancel={onClose} />
				) : (
					<Select options={[{label: 'Close'}]} onSelect={onClose} onCancel={onClose} numbered={false} />
				)}
			</Box>
			<Box marginTop={1}>
				<Text color={theme.secondary}>Enter to confirm · Esc to exit</Text>
			</Box>
		</Box>
	);
}
