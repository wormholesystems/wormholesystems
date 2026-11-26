import { createdId, expect, gotoApp, reloadApp, test } from './fixtures';
import { addSystem, connect, createMap } from './arrange';

// Tabs: a map split over several canvases. Each is worked on like a map of its own; what
// crosses between them is the address, the routes, and what is known about a system.

const JITA = 30000142;
const AMARR = 30002187;
const J122515 = 31001882;

async function addTab(
	api: import('@playwright/test').APIRequestContext,
	mapId: number,
	name: string,
) {
	return createdId(
		await api.post(`/api/maps/${mapId}/tabs/create`, { data: { map_id: mapId, name } }),
	);
}

test('a manager adds a tab, and each tab is its own canvas with its own address', async ({
	page,
	api,
}) => {
	const mapId = await createMap(api, 'E2E Tabs');
	await addSystem(api, mapId, JITA, { x: 200, y: 200 });

	await gotoApp(page, `/maps/${mapId}`);
	const nodes = page.getByTestId('system-node');
	await expect(nodes.filter({ hasText: 'Jita' })).toBeVisible();

	// One tab to begin with, and a way to add another.
	const bar = page.getByTestId('tab-bar');
	await expect(bar.getByTestId('tab')).toHaveText(['Main']);
	await bar.getByTestId('tab-add').click();
	await page.getByTestId('tab-name-input').fill('Thera');
	await page.getByTestId('tab-name-input').press('Enter');

	// The new tab is in view, in the address, and empty: Jita is drawn on Main.
	await expect(bar.getByTestId('tab')).toHaveText(['Main', 'Thera']);
	await expect(bar.getByRole('button', { name: 'Thera' })).toHaveAttribute('aria-pressed', 'true');
	await expect(page).toHaveURL(/[?&]tab=\d+/);
	await expect(nodes).toHaveCount(0);
	const thera = Number(new URL(page.url()).searchParams.get('tab'));

	// What is placed here stays here, and a reload opens on the same tab.
	await addSystem(api, thera, AMARR, { x: 300, y: 200 });
	await expect(nodes.filter({ hasText: 'Amarr' })).toBeVisible();
	await reloadApp(page);
	await expect(bar.getByRole('button', { name: 'Thera' })).toHaveAttribute('aria-pressed', 'true');
	await expect(nodes).toHaveCount(1);
	await expect(nodes.filter({ hasText: 'Amarr' })).toBeVisible();

	// Back on Main there is only what was put on Main, at the bare address.
	await bar.getByRole('button', { name: 'Main' }).click();
	await expect(nodes).toHaveCount(1);
	await expect(nodes.filter({ hasText: 'Jita' })).toBeVisible();
	await expect(page).not.toHaveURL(/[?&]tab=/);
});

test('a system drawn on two tabs links them, and each keeps its own signatures', async ({
	page,
	api,
}) => {
	const mapId = await createMap(api, 'E2E Tab Links');
	const thera = await addTab(api, mapId, 'Thera');
	const jita = await addSystem(api, mapId, JITA, { x: 200, y: 200 });
	const home = await addSystem(api, mapId, J122515, { x: 420, y: 200 });
	await connect(api, mapId, jita, home);
	await addSystem(api, thera, JITA, { x: 300, y: 300, alias: 'Hub' });

	// Scanned on one tab, the signature stays on that tab.
	await api.post(`/api/maps/${mapId}/signatures/add`, {
		data: { map_id: mapId, solar_system_id: JITA, signature_id: 'ABC-123', group: 'data' },
	});
	const onThera = await (await api.get(`/api/maps/${thera}/signatures`)).json();
	expect(onThera).toEqual([]);

	await gotoApp(page, `/maps/${mapId}`);
	const nodes = page.getByTestId('system-node');
	await expect(nodes).toHaveCount(2);
	// Only Jita is on the other tab, and says so. Its name there is that tab's own.
	const link = page.getByTestId('tab-link');
	await expect(link).toHaveCount(1);
	await expect(link).toContainText('Thera');
	await expect(nodes.filter({ hasText: 'Hub' })).toHaveCount(0);

	// Following it goes to the tab, with the system picked there.
	await link.click();
	await expect(page.getByTestId('tab-bar').getByRole('button', { name: 'Thera' })).toHaveAttribute(
		'aria-pressed',
		'true',
	);
	await expect(nodes).toHaveCount(1);
	await expect(nodes.filter({ hasText: 'Hub' })).toBeVisible();
	await expect(page).toHaveURL(new RegExp(`tab=${thera}.*system=${JITA}`));
	// And from here the same system points back.
	await expect(page.getByTestId('tab-link')).toContainText('Main');
});

test('deleting a tab takes what is drawn on it and leaves the rest', async ({ page, api }) => {
	const mapId = await createMap(api, 'E2E Tab Delete');
	const thera = await addTab(api, mapId, 'Thera');
	await addSystem(api, mapId, JITA, { x: 200, y: 200 });
	await addSystem(api, thera, J122515, { x: 300, y: 300 });

	await gotoApp(page, `/maps/${mapId}?tab=${thera}`);
	const bar = page.getByTestId('tab-bar');
	await expect(page.getByTestId('system-node').filter({ hasText: 'J122515' })).toBeVisible();

	await bar.getByTestId('tab-menu').click();
	await page.getByTestId('tab-delete').click();
	await page.getByRole('button', { name: 'Delete tab' }).click();

	// Back on the map's own tab, with what was on it.
	await expect(bar.getByTestId('tab')).toHaveText(['Main']);
	await expect(page.getByTestId('system-node')).toHaveCount(1);
	await expect(page.getByTestId('system-node').filter({ hasText: 'Jita' })).toBeVisible();
	await expect(page).not.toHaveURL(/[?&]tab=/);
});

test('a hole scanned on one tab is drawn on that tab only', async ({ page, api }) => {
	const mapId = await createMap(api, 'E2E Tab Holes');
	await api.post(`/api/maps/${mapId}/update`, {
		data: { map_id: mapId, ghost_unlinked_wormholes: true },
	});
	const hubs = await addTab(api, mapId, 'Hubs');
	await addSystem(api, mapId, JITA, { x: 200, y: 200 });
	await addSystem(api, hubs, JITA, { x: 300, y: 300 });
	await api.post(`/api/maps/${hubs}/signatures/add`, {
		data: { map_id: hubs, solar_system_id: JITA, signature_id: 'ABC-123', group: 'wormhole' },
	});

	// The first tab draws no hole for it, and only says the system is on the other tab too.
	await gotoApp(page, `/maps/${mapId}`);
	const nodes = page.getByTestId('system-node');
	await expect(nodes).toHaveCount(1);
	const link = page.getByTestId('tab-link');
	await expect(link).toHaveText('Hubs');

	// Over there the hole is drawn.
	await link.click();
	await expect(page.getByTestId('tab-bar').getByRole('button', { name: 'Hubs' })).toHaveAttribute(
		'aria-pressed',
		'true',
	);
	await expect(nodes).toHaveCount(2);
});
