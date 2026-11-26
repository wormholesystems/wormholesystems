// The tabs domain: the canvases a map is split over. Each tab is a map of its own
// underneath, so switching is going to another address; this only manages the bar.

import { goto, preloadData } from '$app/navigation';
import { page } from '$app/state';

import { api } from '$lib/api/client';
import type { MapTab } from '$lib/api/types/MapTab';
import type { RunAction } from '$lib/map/actions';
import { tabUrl } from '$lib/map/tabs';

export interface TabsHost {
	/** The tab in view. */
	mapId(): number;
	all(): MapTab[];
	run: RunAction;
}

export class TabsApi {
	constructor(private host: TabsHost) {}

	/** Every tab in bar order. Never empty once the map has loaded. */
	get all(): MapTab[] {
		return this.host.all();
	}

	get current(): MapTab | null {
		return this.all.find((t) => t.id === this.host.mapId()) ?? null;
	}

	/** Go to a tab, optionally with one of its systems made the active one. */
	open(tab: MapTab, system?: number) {
		if (tab.id === this.host.mapId() && system === undefined) return;
		void goto(tabUrl(page.url, tab, system), { keepFocus: true, noScroll: true });
	}

	/** Fetch a tab ahead of the click, so opening it has nothing left to wait for. */
	warm(tab: MapTab) {
		if (tab.id === this.host.mapId()) return;
		const url = tabUrl(page.url, tab);
		void preloadData(`${url.pathname}${url.search}`).catch(() => {});
	}

	/** Add an empty tab at the end of the bar and go to it. */
	create(name: string) {
		void this.host
			.run('createTab', () => api.createTab({ map_id: this.host.mapId(), name }))
			.then((tab) => this.open(tab))
			// Already said by the runner.
			.catch(() => {});
	}

	rename(tab: MapTab, name: string) {
		this.host.run('renameTab', () => api.renameTab({ map_id: tab.id, name }));
	}

	/** Move a tab one place left (`-1`) or right (`1`) in the bar. */
	move(tab: MapTab, by: -1 | 1) {
		const ids = this.all.map((t) => t.id);
		const from = ids.indexOf(tab.id);
		const to = from + by;
		if (from < 0 || to < 0 || to >= ids.length) return;
		[ids[from], ids[to]] = [ids[to], ids[from]];
		this.host.run('reorderTabs', () =>
			api.reorderTabs({ map_id: this.host.mapId(), tab_ids: ids }),
		);
	}

	/** Delete a tab with everything drawn on it. From the tab itself, that leads back to the first. */
	remove(tab: MapTab) {
		const first = this.all.find((t) => t.is_root) ?? null;
		void this.host
			.run('deleteTab', () => api.deleteTab({ map_id: tab.id }))
			.then(() => {
				if (tab.id === this.host.mapId() && first) this.open(first);
			})
			.catch(() => {});
	}
}
