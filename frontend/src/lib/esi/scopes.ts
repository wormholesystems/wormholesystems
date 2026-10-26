// The ESI scopes the app can use, with every surface's copy in one place: `blurb` is the
// settings page's one-liner, `body` the introduction dialog's fuller pitch. The optional
// ones are never asked for at sign-in or in the introduction; they are granted on purpose,
// from the settings page.

import BuildingIcon from '@lucide/svelte/icons/building-2';
import MapPinIcon from '@lucide/svelte/icons/map-pin';
import RouteIcon from '@lucide/svelte/icons/route';
import ShieldIcon from '@lucide/svelte/icons/shield';
import ZapIcon from '@lucide/svelte/icons/zap';
import type { Component } from 'svelte';

export const LOCATION_SCOPE = 'esi-location.read_location.v1';
export const STRUCTURES_SCOPE = 'esi-universe.read_structures.v1';

export interface EsiScope {
	scope: string;
	name: string;
	blurb: string;
	body: string;
	icon: Component;
}

export const ESI_SCOPES: EsiScope[] = [
	{
		scope: LOCATION_SCOPE,
		name: 'Character location',
		blurb: 'Puts you on your system, and measures distances from where you are.',
		body:
			'Where you are. Puts you on your system for everyone on the map, and measures ' +
			'every distance from where you actually are.',
		icon: MapPinIcon,
	},
	{
		scope: 'esi-location.read_online.v1',
		name: 'Online status',
		blurb: 'Stops the map reporting you as somewhere you left hours ago.',
		body:
			'Whether you are logged in, so the map stops reporting you as somewhere you left ' +
			'hours ago.',
		icon: ZapIcon,
	},
	{
		scope: 'esi-location.read_ship_type.v1',
		name: 'Ship type',
		blurb: 'Shows what you are flying, not just that you are there.',
		body:
			'What you are flying. The difference between "someone is in the hole" and ' +
			'"a Loki is in the hole".',
		icon: ShieldIcon,
	},
	{
		scope: 'esi-ui.write_waypoint.v1',
		name: 'Set waypoints',
		blurb: 'Lets the map put a destination straight into your client.',
		body:
			'Lets the map put a destination straight into your client, instead of you retyping ' +
			'system names.',
		icon: RouteIcon,
	},
];

export const OPTIONAL_SCOPES: EsiScope[] = [
	{
		scope: STRUCTURES_SCOPE,
		name: 'Structure names',
		blurb: 'Names the structures you dock in, private ones included, for everyone on the map.',
		body:
			'Names the structures you dock in. Public ones are named anyway; a private one only ' +
			'EVE shows to pilots on its docking list, so the map names it once one of them ' +
			'with this permission docks there.',
		icon: BuildingIcon,
	},
];

export function scopeMeta(scope: string): EsiScope | undefined {
	return [...ESI_SCOPES, ...OPTIONAL_SCOPES].find((s) => s.scope === scope);
}
