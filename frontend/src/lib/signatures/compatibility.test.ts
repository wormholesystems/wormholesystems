import { describe, expect, it } from 'vitest';

import { canBeConnection, canLeadToClass, groupSignatures } from './compatibility';
import { C1_TO_3, C4_TO_5 } from '$lib/map/classes';
import type { Signature } from '$lib/api/types/Signature';
import type { SignatureGroup } from '$lib/api/types/SignatureGroup';
import type { SignatureTypeInfo } from '$lib/api/types/SignatureTypeInfo';

function sig(
	signature_id: string,
	overrides: Partial<Signature> & { group?: SignatureGroup } = {},
): Signature {
	return {
		id: 1,
		map_id: 1,
		solar_system_id: 1,
		signature_id,
		group: 'wormhole',
		signature_type_id: null,
		name: null,
		size: null,
		mass_status: null,
		time_status: null,
		time_status_updated_at: null,
		connection_id: null,
		created_at: '',
		updated_at: '',
		...overrides,
	};
}

function type(id: number, target_class: number | null): SignatureTypeInfo {
	return {
		id,
		signature: 'H296',
		name: 'Wormhole H296',
		signature_category_id: 1,
		target_class,
		extra: null,
		spawn_areas: [],
		regions: [],
		constellations: [],
		systems: [],
		total_mass: null,
		max_jump_mass: null,
		lifetime_hours: null,
		signature_strength: null,
	};
}

// A K162 leads back to wherever the other side spawned, so its class is open.
const K162 = type(1, null);
const TO_C5 = type(2, 5);
const TO_NULLSEC = type(3, 9);

const types = new Map([
	[K162.id, K162],
	[TO_C5.id, TO_C5],
	[TO_NULLSEC.id, TO_NULLSEC],
]);

describe('canBeConnection', () => {
	it('accepts wormholes and anything not yet classified', () => {
		expect(canBeConnection(sig('ABC-123', { group: 'wormhole' }))).toBe(true);
		expect(canBeConnection(sig('ABC-123', { group: 'unknown' }))).toBe(true);
	});

	it('rejects sites, which are never a way out of the system', () => {
		for (const group of [
			'data',
			'relic',
			'gas',
			'combat',
			'ore',
			'homefront',
			'faction_warfare',
		] as const) {
			expect(canBeConnection(sig('ABC-123', { group }))).toBe(false);
		}
	});
});

describe('canLeadToClass', () => {
	it('holds a typed hole to its destination class', () => {
		expect(canLeadToClass(TO_C5, 5)).toBe(true);
		expect(canLeadToClass(TO_C5, 4)).toBe(false);
	});

	it('lets an open or unresolved type lead anywhere', () => {
		expect(canLeadToClass(K162, 4)).toBe(true);
		expect(canLeadToClass(null, 4)).toBe(true);
		expect(canLeadToClass(undefined, 4)).toBe(true);
	});

	it('lets a K162 group lead to each of its classes', () => {
		const toC1To3 = type(4, C1_TO_3);
		expect(canLeadToClass(toC1To3, 2)).toBe(true);
		expect(canLeadToClass(toC1To3, 4)).toBe(false);
		const toC4To5 = type(5, C4_TO_5);
		expect(canLeadToClass(toC4To5, 5)).toBe(true);
		expect(canLeadToClass(toC4To5, 6)).toBe(false);
	});

	it('lets anything lead somewhere unknown', () => {
		expect(canLeadToClass(TO_C5, null)).toBe(true);
	});
});

describe('groupSignatures', () => {
	it('splits by what the map already knows, in scanner order', () => {
		const groups = groupSignatures(
			[
				sig('XYZ-100', { id: 1, signature_type_id: K162.id }),
				sig('DEF-200', { id: 2, group: 'gas' }),
				sig('ABC-300', { id: 3, signature_type_id: TO_NULLSEC.id }),
				sig('GHI-400', { id: 4, connection_id: 7 }),
				sig('BCD-500', { id: 5, group: 'unknown' }),
			],
			types,
			5,
		);

		expect(groups.likely.map((s) => s.signature_id)).toEqual(['BCD-500', 'XYZ-100']);
		expect(groups.connected.map((s) => s.signature_id)).toEqual(['GHI-400']);
		// Typed as a nullsec hole, so it cannot be the one that led into a C5.
		expect(groups.unlikely.map((s) => s.signature_id)).toEqual(['ABC-300']);
	});

	it('demotes rather than hides, so a mistyped signature stays pickable', () => {
		const mistyped = sig('ABC-123', { signature_type_id: TO_NULLSEC.id });
		const groups = groupSignatures([mistyped], types, 5);
		const all = [...groups.likely, ...groups.connected, ...groups.unlikely];
		expect(all).toHaveLength(1);
	});

	it('drops sites entirely', () => {
		const groups = groupSignatures([sig('ABC-123', { group: 'relic' })], types, 5);
		expect([...groups.likely, ...groups.connected, ...groups.unlikely]).toHaveLength(0);
	});
});
