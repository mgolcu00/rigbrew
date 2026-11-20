import { readFileSync } from 'node:fs';
import { homedir } from 'node:os';
import { join } from 'node:path';
import { fileURLToPath } from 'node:url';
import adapter from '@sveltejs/adapter-static';
import { sveltekit } from '@sveltejs/kit/vite';
import { defineConfig, searchForWorkspaceRoot, type Plugin } from 'vite';

const version: string = JSON.parse(readFileSync(new URL('./package.json', import.meta.url), 'utf8')).version;

/** Where rigbrewd writes its port and token (see daemon/src/main.rs). */
function daemonFile() {
	const root =
		process.env.RIGBREW_DATA_DIR ??
		(process.platform === 'darwin'
			? join(homedir(), 'Library', 'Application Support', 'Rigbrew')
			: process.platform === 'win32'
				? join(process.env.APPDATA ?? homedir(), 'Rigbrew')
				: join(process.env.XDG_DATA_HOME ?? join(homedir(), '.local', 'share'), 'Rigbrew'));
	return join(root, 'daemon.json');
}

/**
 * Browser preview only: hands the page the service's current port and token, the way the
 * native side does in the app. Read on every call, so the preview follows a restarted service.
 * Same-origin only (no CORS header), and only on the dev server.
 */
const daemonInfo = (): Plugin => ({
	name: 'rigbrew-daemon-info',
	apply: 'serve',
	configureServer(server) {
		// Development only: `curl 'localhost:1420/__view?screen=models&theme=dark'` puts every
		// open window (the native one too) on that screen, to look at it or take a screenshot.
		server.middlewares.use('/__view', (req, res) => {
			const view = Object.fromEntries(new URL(req.url ?? '', 'http://localhost').searchParams);
			server.ws.send({ type: 'custom', event: 'rigbrew:view', data: view });
			res.end('ok');
		});
		server.middlewares.use('/__daemon', (_req, res) => {
			res.setHeader('Content-Type', 'application/json');
			res.setHeader('Cache-Control', 'no-store');
			try {
				const { port, token } = JSON.parse(readFileSync(daemonFile(), 'utf8'));
				res.end(JSON.stringify({ port, token }));
			} catch {
				res.statusCode = 404;
				res.end('{}');
			}
		});
	}
});

// Accounts (Firebase sign-in, the server's account routes) are one folder behind one import,
// `#account`. `RIGBREW_ACCOUNT=1` builds with `src/account`; anything else builds with
// `src/account-stub`, which reports "no account" and pulls in no Firebase code.
const accountEntry = fileURLToPath(new URL(process.env.RIGBREW_ACCOUNT === '1' ? './src/account/index.ts' : './src/account-stub/index.ts', import.meta.url));

// rigbrewd only accepts requests from the app's own origins, and in development that is
// http://localhost:1420. The port is therefore fixed.
export default defineConfig({
	plugins: [daemonInfo(), sveltekit({ adapter: adapter({ fallback: 'index.html' }) })],
	resolve: { alias: [{ find: /^#account$/, replacement: accountEntry }] },
	clearScreen: false,
	// The legal texts are imported from the repository's docs/legal (src/lib/legal.ts).
	server: { port: 1420, strictPort: true, host: 'localhost', fs: { allow: [searchForWorkspaceRoot(process.cwd()), fileURLToPath(new URL('../docs/legal', import.meta.url))] } },
	envPrefix: ['VITE_'],
	define: { __APP_VERSION__: JSON.stringify(version) }
});
