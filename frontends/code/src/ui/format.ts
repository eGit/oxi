export function formatDuration(secs: number): string {
	secs = Math.max(0, Math.round(secs));
	if (secs < 60) return `${secs}s`;
	const h = Math.floor(secs / 3600);
	const m = Math.floor((secs % 3600) / 60);
	const s = secs % 60;
	if (h) return `${h}h ${m}m ${s}s`;
	return `${m}m ${s}s`;
}

export function formatTokens(n: number): string {
	if (n < 1000) return String(Math.round(n));
	if (n < 1_000_000) return `${(n / 1000).toFixed(1)}k`;
	return `${(n / 1_000_000).toFixed(1)}m`;
}

export function formatAge(unixSecs: number, now = Date.now() / 1000): string {
	const d = Math.max(0, now - unixSecs);
	const unit = (n: number, word: string) => `${n} ${word}${n === 1 ? '' : 's'} ago`;
	if (d < 60) return 'just now';
	if (d < 3600) return unit(Math.floor(d / 60), 'minute');
	if (d < 86400) return unit(Math.floor(d / 3600), 'hour');
	if (d < 86400 * 30) return unit(Math.floor(d / 86400), 'day');
	if (d < 86400 * 365) return unit(Math.floor(d / (86400 * 30)), 'month');
	return unit(Math.floor(d / (86400 * 365)), 'year');
}

export function formatResetsIn(secs: number): string {
	const h = Math.floor(secs / 3600);
	const m = Math.floor((secs % 3600) / 60);
	if (h >= 24) return `${Math.floor(h / 24)}d ${h % 24}h`;
	if (h) return `${h}h ${m}m`;
	return `${m}m`;
}

/** `now`, `5m`, `3h`, `2d`: ages in a narrow column. */
export function formatAgeShort(unixSecs: number, now = Date.now() / 1000): string {
	const d = Math.max(0, now - unixSecs);
	if (d < 60) return 'now';
	if (d < 3600) return `${Math.floor(d / 60)}m`;
	if (d < 86400) return `${Math.floor(d / 3600)}h`;
	if (d < 86400 * 365) return `${Math.floor(d / 86400)}d`;
	return `${Math.floor(d / (86400 * 365))}y`;
}
