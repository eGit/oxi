// Claude Code's dark theme colors.

import chalk from 'chalk';

export const theme = {
	claude: 'rgb(215,119,87)',
	claudeShimmer: 'rgb(245,149,117)',
	text: 'rgb(255,255,255)',
	secondary: 'rgb(153,153,153)',
	subtle: 'rgb(80,80,80)',
	promptBorder: 'rgb(136,136,136)',
	suggestion: 'rgb(177,185,249)',
	permission: 'rgb(177,185,249)',
	remember: 'rgb(177,185,249)',
	planMode: 'rgb(72,150,140)',
	autoAccept: 'rgb(175,135,255)',
	bashBorder: 'rgb(253,93,177)',
	success: 'rgb(78,186,101)',
	error: 'rgb(255,107,128)',
	warning: 'rgb(255,193,7)',
	diffAdded: 'rgb(34,92,43)',
	diffRemoved: 'rgb(122,41,54)',
	diffAddedWord: 'rgb(56,166,96)',
	diffRemovedWord: 'rgb(179,89,107)',
	userMessageBackground: 'rgb(55,55,55)',
} as const;

const rgb = (color: string) => {
	const [r, g, b] = color.match(/\d+/g)!.map(Number);
	return [r, g, b] as const;
};

export const fg = (color: string) => chalk.rgb(...rgb(color));
export const bg = (color: string) => chalk.bgRgb(...rgb(color));

export const c = {
	claude: fg(theme.claude),
	secondary: fg(theme.secondary),
	suggestion: fg(theme.suggestion),
	permission: fg(theme.permission),
	success: fg(theme.success),
	error: fg(theme.error),
	warning: fg(theme.warning),
	planMode: fg(theme.planMode),
	autoAccept: fg(theme.autoAccept),
	bash: fg(theme.bashBorder),
};
