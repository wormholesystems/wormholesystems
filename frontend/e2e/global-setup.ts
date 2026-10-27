import { chromium, request } from '@playwright/test';

import { E2E_CHARACTER_ID, E2E_SESSION, ensureIdentity } from './db';
import { createdId } from './fixtures';

// Create (or refresh) the main e2e identity every spec signs in as by default, then warm
// up the dev server.
export default async function globalSetup() {
	await ensureIdentity({
		characterId: E2E_CHARACTER_ID,
		session: E2E_SESSION,
		name: 'E2E Pilot',
		ownerHash: 'e2e-owner-hash',
	});
	await warmUp();
}

/**
 * A cold dev server compiles every page on its first request, and reloads a page halfway
 * through when it finds dependencies to bundle. On a fresh checkout that is longer than a
 * spec waits, so the first specs used to fail on hydration alone. One visit to each kind of
 * page, given all the time it needs, takes that out of the specs.
 */
async function warmUp() {
	const api = await request.newContext({
		baseURL: 'http://127.0.0.1:3000',
		extraHTTPHeaders: { cookie: `ws_session=${E2E_SESSION}` },
	});
	const mapId = await createdId(await api.post('/api/maps', { data: { name: 'E2E Warm-up' } }));

	const browser = await chromium.launch();
	const context = await browser.newContext({ baseURL: 'http://localhost:5173' });
	await context.addCookies([
		{ name: 'ws_session', value: E2E_SESSION, domain: 'localhost', path: '/' },
	]);
	const page = await context.newPage();
	for (const path of ['/', '/maps', `/maps/${mapId}`, `/maps/${mapId}/settings`]) {
		await page.goto(path, { timeout: 180_000 });
		await page.waitForSelector('html[data-hydrated="true"]', { timeout: 180_000 });
	}
	await browser.close();

	await api.delete(`/api/maps/${mapId}`);
	await api.dispose();
}
