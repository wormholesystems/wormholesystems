import { describe, expect, it } from 'vitest';

import type { MapTab } from '$lib/api/types/MapTab';
import type { TabNetwork } from '$lib/api/types/TabNetwork';
import { elsewhereEdges, tabFromUrl, tabsBySystem, tabUrl } from './tabs';

const main: MapTab = { id: 7, name: 'Main', is_root: true };
const thera: MapTab = { id: 12, name: 'Thera', is_root: false };
const turnur: MapTab = { id: 13, name: 'Turnur', is_root: false };

const JITA = 30000142;
const THERA = 31000005;
const AMARR = 30002187;

const elsewhere: TabNetwork[] = [
	{
		tab_id: thera.id,
		systems: [THERA, JITA],
		connections: [
			{
				from_solar_system_id: THERA,
				to_solar_system_id: JITA,
				kind: 'wormhole',
				mass_status: 'reduced',
				time_status: 'eol',
			},
			{
				from_solar_system_id: JITA,
				to_solar_system_id: AMARR,
				kind: 'stargate',
				mass_status: null,
				time_status: null,
			},
		],
	},
	{
		tab_id: turnur.id,
		systems: [JITA],
		connections: [],
	},
];

describe('the tab in the address', () => {
	it('reads the tab, and nothing that is not one', () => {
		expect(tabFromUrl(new URL('https://x.test/maps/home-7?tab=12'))).toBe(12);
		expect(tabFromUrl(new URL('https://x.test/maps/home-7'))).toBeNull();
		expect(tabFromUrl(new URL('https://x.test/maps/home-7?tab=thera'))).toBeNull();
		expect(tabFromUrl(new URL('https://x.test/maps/home-7?tab=-3'))).toBeNull();
	});

	it('names a tab, and leaves the map itself as the bare address', () => {
		const from = new URL('https://x.test/maps/home-7?system=30000142&panel=sigs');
		const to = tabUrl(from, thera);
		expect(to.searchParams.get('tab')).toBe('12');
		// The system that was active may not be on the other tab.
		expect(to.searchParams.has('system')).toBe(false);
		expect(to.searchParams.get('panel')).toBe('sigs');

		const back = tabUrl(to, main);
		expect(back.searchParams.has('tab')).toBe(false);
	});

	it('carries a system when the link is to that system on the tab', () => {
		const to = tabUrl(new URL('https://x.test/maps/home-7'), thera, JITA);
		expect(to.search).toBe('?tab=12&system=30000142');
	});
});

describe('what the other tabs draw', () => {
	it('adds their connections to the routes, under the same switches as this tab', () => {
		expect(elsewhereEdges(elsewhere, true)).toEqual([
			{ a: THERA, b: JITA, via: 'wormhole', mass: 'reduced', time: 'eol' },
			{ a: JITA, b: AMARR, via: 'stargate', mass: null, time: null },
		]);
		// Wormholes switched off leaves the drawn gate jumps, as it does here.
		expect(elsewhereEdges(elsewhere, false).map((e) => e.via)).toEqual(['stargate']);
	});

	it('says which other tabs a system is on, in bar order', () => {
		const by = tabsBySystem(elsewhere, [main, turnur, thera]);
		expect(by.get(JITA)?.map((t) => t.name)).toEqual(['Turnur', 'Thera']);
		expect(by.get(THERA)?.map((t) => t.name)).toEqual(['Thera']);
		expect(by.has(AMARR)).toBe(false);
	});
});
