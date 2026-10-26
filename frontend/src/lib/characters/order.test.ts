import { describe, expect, it } from 'vitest';

import { COVERT_OPS_GROUP, isIdle, orderPilots } from './order';
import type { MapCharacter } from '$lib/api/types/MapCharacter';

function pilot(name: string, overrides: Partial<MapCharacter> = {}): MapCharacter {
	return {
		character_id: name.length,
		name,
		corporation_ticker: 'E2E',
		solar_system_id: 30000142,
		ship_type_id: 17738,
		ship_name: 'A ship',
		ship_type: 'Machariel',
		ship_group_id: 27,
		is_docked: false,
		docked_at: null,
		is_mine: false,
		...overrides,
	};
}

const pod = pilot('Podded Pete', { ship_type: 'Capsule' });
const docked = pilot('Docked Dana', { is_docked: true });
const scanner = pilot('Scanning Sam', { ship_group_id: COVERT_OPS_GROUP });
const ready = pilot('Ready Rita');
const alsoReady = pilot('Able Abe');

describe('orderPilots', () => {
	it('puts whoever can act first, and the pod last', () => {
		const order = orderPilots([pod, docked, scanner, ready]).map((p) => p.name);
		expect(order).toEqual(['Ready Rita', 'Scanning Sam', 'Docked Dana', 'Podded Pete']);
	});

	it('breaks ties by name', () => {
		expect(orderPilots([ready, alsoReady]).map((p) => p.name)).toEqual(['Able Abe', 'Ready Rita']);
	});

	it('hides nobody: a docked alt is still on the list', () => {
		expect(orderPilots([pod, docked, scanner, ready])).toHaveLength(4);
	});

	it('does not mutate the input, which is reactive state', () => {
		const input = [pod, ready];
		orderPilots(input);
		expect(input.map((p) => p.name)).toEqual(['Podded Pete', 'Ready Rita']);
	});

	it('a docked pod sorts as a pod, since it is the worse signal', () => {
		const dockedPod = pilot('Aaa', { ship_type: 'Capsule', is_docked: true });
		expect(orderPilots([dockedPod, docked]).map((p) => p.name)).toEqual(['Docked Dana', 'Aaa']);
	});
});

describe('isIdle', () => {
	it('dims docked pilots and pods, not scanners', () => {
		expect(isIdle(docked)).toBe(true);
		expect(isIdle(pod)).toBe(true);
		// A scanner is working, not idle: it sinks in the order but stays legible.
		expect(isIdle(scanner)).toBe(false);
		expect(isIdle(ready)).toBe(false);
	});
});
