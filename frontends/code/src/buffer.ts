// The prompt's text and cursor, and the edits the keyboard makes to them. Pure functions.

export type Buffer = {text: string; cursor: number};

export const empty: Buffer = {text: '', cursor: 0};

const clamp = (n: number, lo: number, hi: number) => Math.max(lo, Math.min(hi, n));

export function insert(b: Buffer, s: string): Buffer {
	return {text: b.text.slice(0, b.cursor) + s + b.text.slice(b.cursor), cursor: b.cursor + s.length};
}

/** Remove `[from, to)` and put the cursor at `from`. */
function cut(b: Buffer, from: number, to: number): Buffer {
	from = clamp(from, 0, b.text.length);
	to = clamp(to, 0, b.text.length);
	if (to < from) [from, to] = [to, from];
	return {text: b.text.slice(0, from) + b.text.slice(to), cursor: from};
}

/** Code-point aware step left/right, so emoji and surrogate pairs move as one. */
function prevIndex(text: string, i: number): number {
	if (i <= 0) return 0;
	const code = text.charCodeAt(i - 1);
	return code >= 0xdc00 && code <= 0xdfff && i >= 2 ? i - 2 : i - 1;
}

function nextIndex(text: string, i: number): number {
	if (i >= text.length) return text.length;
	const code = text.charCodeAt(i);
	return code >= 0xd800 && code <= 0xdbff && i + 1 < text.length ? i + 2 : i + 1;
}

export const backspace = (b: Buffer): Buffer => cut(b, prevIndex(b.text, b.cursor), b.cursor);
export const deleteForward = (b: Buffer): Buffer => cut(b, b.cursor, nextIndex(b.text, b.cursor));
export const left = (b: Buffer): Buffer => ({...b, cursor: prevIndex(b.text, b.cursor)});
export const right = (b: Buffer): Buffer => ({...b, cursor: nextIndex(b.text, b.cursor)});

const lineStart = (text: string, i: number) => text.lastIndexOf('\n', i - 1) + 1;
const lineEnd = (text: string, i: number) => {
	const end = text.indexOf('\n', i);
	return end === -1 ? text.length : end;
};

export const home = (b: Buffer): Buffer => ({...b, cursor: lineStart(b.text, b.cursor)});
export const end = (b: Buffer): Buffer => ({...b, cursor: lineEnd(b.text, b.cursor)});

/** Ctrl+K: to the end of the line, or the newline when already there. */
export function killToEnd(b: Buffer): Buffer {
	const endAt = lineEnd(b.text, b.cursor);
	return cut(b, b.cursor, endAt === b.cursor ? endAt + 1 : endAt);
}

export const killToStart = (b: Buffer): Buffer => cut(b, lineStart(b.text, b.cursor), b.cursor);

const isWord = (ch: string) => /[\p{L}\p{N}_]/u.test(ch);

function wordStartBefore(text: string, i: number): number {
	while (i > 0 && !isWord(text[i - 1])) i--;
	while (i > 0 && isWord(text[i - 1])) i--;
	return i;
}

function wordEndAfter(text: string, i: number): number {
	while (i < text.length && !isWord(text[i])) i++;
	while (i < text.length && isWord(text[i])) i++;
	return i;
}

export const wordLeft = (b: Buffer): Buffer => ({...b, cursor: wordStartBefore(b.text, b.cursor)});
export const wordRight = (b: Buffer): Buffer => ({...b, cursor: wordEndAfter(b.text, b.cursor)});
export const deleteWordBack = (b: Buffer): Buffer => cut(b, wordStartBefore(b.text, b.cursor), b.cursor);
export const deleteWordForward = (b: Buffer): Buffer => cut(b, b.cursor, wordEndAfter(b.text, b.cursor));

export const onFirstLine = (b: Buffer) => b.text.lastIndexOf('\n', b.cursor - 1) === -1;
export const onLastLine = (b: Buffer) => b.text.indexOf('\n', b.cursor) === -1;

/** Up/down within the text, keeping the column. */
export function vertical(b: Buffer, dir: -1 | 1): Buffer {
	const start = lineStart(b.text, b.cursor);
	const column = b.cursor - start;
	if (dir < 0) {
		if (start === 0) return {...b, cursor: 0};
		const prevStart = lineStart(b.text, start - 1);
		return {...b, cursor: Math.min(prevStart + column, start - 1)};
	}
	const endAt = lineEnd(b.text, b.cursor);
	if (endAt === b.text.length) return {...b, cursor: b.text.length};
	const nextStart = endAt + 1;
	return {...b, cursor: Math.min(nextStart + column, lineEnd(b.text, nextStart))};
}

export const set = (text: string): Buffer => ({text, cursor: text.length});
