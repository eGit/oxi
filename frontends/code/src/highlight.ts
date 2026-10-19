// Syntax highlighting for code blocks: highlight.js classes mapped to ANSI colors.

import chalk from 'chalk';
import hljs from 'highlight.js/lib/common';

const styles: Record<string, (text: string) => string> = {
	keyword: chalk.rgb(198, 120, 221),
	built_in: chalk.rgb(229, 192, 123),
	type: chalk.rgb(229, 192, 123),
	literal: chalk.rgb(86, 182, 194),
	number: chalk.rgb(209, 154, 102),
	string: chalk.rgb(152, 195, 121),
	regexp: chalk.rgb(152, 195, 121),
	symbol: chalk.rgb(86, 182, 194),
	comment: chalk.rgb(127, 132, 142).italic,
	doctag: chalk.rgb(198, 120, 221),
	meta: chalk.rgb(97, 175, 239),
	title: chalk.rgb(97, 175, 239),
	'title.class': chalk.rgb(229, 192, 123),
	'title.function': chalk.rgb(97, 175, 239),
	params: chalk.rgb(224, 108, 117),
	attr: chalk.rgb(209, 154, 102),
	attribute: chalk.rgb(152, 195, 121),
	variable: chalk.rgb(224, 108, 117),
	property: chalk.rgb(224, 108, 117),
	tag: chalk.rgb(224, 108, 117),
	name: chalk.rgb(224, 108, 117),
	'selector-tag': chalk.rgb(224, 108, 117),
	'selector-class': chalk.rgb(209, 154, 102),
	'selector-id': chalk.rgb(97, 175, 239),
	addition: chalk.rgb(152, 195, 121),
	deletion: chalk.rgb(224, 108, 117),
	section: chalk.rgb(224, 108, 117).bold,
	bullet: chalk.rgb(86, 182, 194),
	emphasis: chalk.italic,
	strong: chalk.bold,
	link: chalk.underline,
	subst: (text: string) => text,
};

const decode = (text: string) =>
	text
		.replace(/&lt;/g, '<')
		.replace(/&gt;/g, '>')
		.replace(/&quot;/g, '"')
		.replace(/&#x27;/g, "'")
		.replace(/&#39;/g, "'")
		.replace(/&amp;/g, '&');

/** Turn highlight.js HTML into ANSI text. Spans nest, so styles stack. */
function htmlToAnsi(html: string): string {
	const stack: Array<(text: string) => string> = [];
	let out = '';
	const re = /<span class="([^"]+)">|<\/span>|([^<]+)/g;
	let match: RegExpExecArray | null;
	while ((match = re.exec(html))) {
		if (match[1] !== undefined) {
			const names = match[1].split(/\s+/).map(name => name.replace(/^hljs-/, '').replace(/_$/, ''));
			const key = names.join('.');
			stack.push(styles[key] ?? styles[names[0]] ?? (text => text));
		} else if (match[0] === '</span>') {
			stack.pop();
		} else {
			let text = decode(match[2]);
			for (let i = stack.length - 1; i >= 0; i--) text = stack[i](text);
			out += text;
		}
	}
	return out;
}

export function highlight(code: string, language?: string): string {
	const lang = language?.trim().toLowerCase();
	try {
		if (lang && hljs.getLanguage(lang)) {
			return htmlToAnsi(hljs.highlight(code, {language: lang, ignoreIllegals: true}).value);
		}
		if (!lang && code.length < 20_000) {
			return htmlToAnsi(hljs.highlightAuto(code).value);
		}
	} catch {
		// Fall through to plain text.
	}
	return code;
}

/** Language from a file extension, for highlighting diffs and file previews. */
export function languageForPath(file: string): string | undefined {
	const ext = file.split('.').pop()?.toLowerCase();
	const map: Record<string, string> = {
		rs: 'rust', ts: 'typescript', tsx: 'typescript', js: 'javascript', jsx: 'javascript', mjs: 'javascript',
		py: 'python', rb: 'ruby', go: 'go', java: 'java', kt: 'kotlin', swift: 'swift', c: 'c', h: 'c',
		cpp: 'cpp', cc: 'cpp', hpp: 'cpp', cs: 'csharp', sh: 'bash', zsh: 'bash', bash: 'bash',
		json: 'json', toml: 'ini', yaml: 'yaml', yml: 'yaml', md: 'markdown', html: 'xml', xml: 'xml',
		css: 'css', scss: 'scss', sql: 'sql', lua: 'lua', php: 'php',
	};
	return ext ? map[ext] : undefined;
}
