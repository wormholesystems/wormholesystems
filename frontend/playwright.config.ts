import { defineConfig } from '@playwright/test';

// The suite has a database of its own, so it neither reads nor writes the one you develop
// against, and a branch with another migration there does not stop it booting. Set here,
// before anything starts: the API and the fixtures (`e2e/db.ts`) both read it.
const devDatabase =
	process.env.DATABASE_URL ??
	'postgres://wormholesystems:wormholesystems@localhost:5432/wormholesystems';
process.env.DATABASE_URL =
	process.env.E2E_DATABASE_URL ?? devDatabase.replace(/\/[^/?]+(\?.*)?$/, '/wormholesystems_e2e$1');

// E2E tests run against the real stack: Postgres (docker compose up db), the Rust API,
// and the vite dev server, on a database of their own (below). Both servers are reused
// when already running, so an API you started yourself must point at that database too.
export default defineConfig({
	testDir: 'e2e',
	// The suite shares one dev stack: six workers against one debug-build API, one Postgres
	// and one dev server. Nothing here is slow on its own — every one of these passes in
	// isolation in milliseconds — but a round trip under that load can outlast the default
	// five seconds an assertion waits, which is what used to show up as flakiness. The
	// budget is the shared stack's, not the app's.
	expect: { timeout: 10_000 },
	// Half of Playwright's default. The stack under test is one debug-build API, one dev
	// server and one Postgres, and six workers oversubscribe it badly enough that
	// assertions start losing races that have nothing to do with the code. Fewer workers
	// costs about a minute and buys a suite that means something when it goes red.
	workers: 3,
	// Kept at zero deliberately: a retry hides exactly the intermittency worth knowing
	// about. If this suite goes red, something is actually wrong.
	retries: 0,
	use: {
		baseURL: 'http://localhost:5173',
	},
	globalSetup: './e2e/global-setup',
	globalTeardown: './e2e/global-teardown',
	projects: [
		{ name: 'app', testIgnore: /esi-driven\.spec\.ts/ },
		{
			// Tranquility's status is process-global: taking the server down affects every
			// request the API makes, so these run on their own once the rest is finished.
			name: 'esi-driven',
			testMatch: /esi-driven\.spec\.ts/,
			dependencies: ['app'],
		},
	],
	webServer: [
		{
			// Stands in for ESI so a pilot can be flown around without a live EVE session.
			// Unscripted characters are proxied to the real thing, so it is harmless in
			// front of a dev stack.
			command: 'node e2e/esi-stub.mjs',
			url: 'http://127.0.0.1:3999/_stub/hits/0',
			reuseExistingServer: true,
			timeout: 30_000,
		},
		{
			// The API migrates and seeds its database on boot, but cannot create it.
			command: 'node frontend/e2e/ensure-db.mjs && cargo run',
			cwd: '..',
			url: 'http://127.0.0.1:3000/api/grid-config',
			reuseExistingServer: true,
			env: {
				DATABASE_URL: process.env.DATABASE_URL,
				// Compiled against the query cache, as CI does: sqlx would otherwise check the
				// queries against the e2e database, which has not yet seen a branch's new
				// migrations when the build runs.
				SQLX_OFFLINE: 'true',
				ESI_BASE_URL: 'http://127.0.0.1:3999',
				// Tight enough that a test can take Tranquility down and see the effect.
				SERVER_STATUS_POLL_SECS: '2',
				SKYHOOK_POLL_SECS: '2',
				// Tests seed the killmails they assert on; a real 90-day import would download
				// several gigabytes before the first spec ran.
				KILLMAIL_BACKFILL_DAYS: '0',
				EVE_SCOUT_URL: 'http://127.0.0.1:3999/evescout',
			},
			// First run compiles the API and may seed the SDE into a fresh database.
			timeout: 300_000,
		},
		{
			command: 'npm run dev',
			url: 'http://localhost:5173',
			reuseExistingServer: true,
			timeout: 60_000,
		},
	],
});
