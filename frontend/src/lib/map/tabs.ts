// Tabs: a map split over several canvases. Each tab is a map of its own underneath, with
// its own systems, connections and history, so the page simply works on the tab's id.
// What is left for the client to join up is the little that crosses tabs: the address,
// the routes, and which systems another tab draws too.

import type { MapTab } from '$lib/api/types/MapTab';
import type { TabNetwork } from '$lib/api/types/TabNetwork';
import type { DynamicEdge } from '$lib/routing/algorithm';

/** The tab `?tab=` names, or null when the address names none. */
export function tabFromUrl(url: URL): number | null {
	const tab = Number(url.searchParams.get('tab'));
	return Number.isSafeInteger(tab) && tab > 0 ? tab : null;
}

/**
 * The address of a tab of the map at `url`. The map's own first tab is the bare address,
 * so a map nobody has split keeps the links it always had. The active system is dropped:
 * another tab may not draw it, and a link that names both says so on purpose.
 */
export function tabUrl(url: URL, tab: MapTab, system?: number): URL {
	const next = new URL(url);
	next.searchParams.delete('system');
	if (tab.is_root) next.searchParams.delete('tab');
	else next.searchParams.set('tab', String(tab.id));
	if (system !== undefined) next.searchParams.set('system', String(system));
	return next;
}

/**
 * The other tabs' connections as route edges. One network for the pathfinder, however
 * many canvases it is drawn on: a route may leave this tab through a system both draw.
 */
export function elsewhereEdges(
	elsewhere: readonly TabNetwork[],
	useWormholes: boolean,
): DynamicEdge[] {
	const edges: DynamicEdge[] = [];
	for (const tab of elsewhere) {
		for (const c of tab.connections) {
			if (c.from_solar_system_id === c.to_solar_system_id) continue;
			if (c.kind === 'wormhole') {
				if (!useWormholes) continue;
				edges.push({
					a: c.from_solar_system_id,
					b: c.to_solar_system_id,
					via: 'wormhole',
					mass: c.mass_status,
					time: c.time_status,
				});
			} else {
				edges.push({
					a: c.from_solar_system_id,
					b: c.to_solar_system_id,
					via: 'stargate',
					mass: null,
					time: null,
				});
			}
		}
	}
	return edges;
}

/** The other tabs that draw each solar system, in bar order: where a node also lives. */
export function tabsBySystem(
	elsewhere: readonly TabNetwork[],
	tabs: readonly MapTab[],
): Map<number, MapTab[]> {
	const out = new Map<number, MapTab[]>();
	for (const tab of tabs) {
		const network = elsewhere.find((n) => n.tab_id === tab.id);
		if (!network) continue;
		for (const system of network.systems) {
			const list = out.get(system);
			if (list) list.push(tab);
			else out.set(system, [tab]);
		}
	}
	return out;
}
