// Bundles oxi-code into one file, dist/oxi-code.js, that runs with plain `node`.
import {build} from 'esbuild';

// Ink imports React devtools, used only with DEV=true; an empty stand-in keeps them out.
const devtoolsStub = {
	name: 'devtools-stub',
	setup(b) {
		b.onResolve({filter: /^react-devtools-core$/}, () => ({path: 'stub', namespace: 'devtools-stub'}));
		b.onLoad({filter: /.*/, namespace: 'devtools-stub'}, () => ({contents: 'export default {connectToDevTools() {}};'}));
	},
};

await build({
	entryPoints: ['src/main.tsx'],
	bundle: true,
	platform: 'node',
	format: 'esm',
	target: 'node22',
	outfile: 'dist/oxi-code.js',
	jsx: 'automatic',
	plugins: [devtoolsStub],
	define: {'process.env.NODE_ENV': '"production"'},
	banner: {js: "import {createRequire as __createRequire} from 'node:module'; const require = __createRequire(import.meta.url);"},
	logLevel: 'warning',
});
