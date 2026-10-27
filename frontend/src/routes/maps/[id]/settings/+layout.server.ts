import { redirect } from '@sveltejs/kit';

import { mapIdFromSlug, mapSlug } from '$lib/map/slug';
import { mapView } from '$lib/server/api';
import { mapLoadFailed, offline } from '$lib/server/load-errors';
import type { LayoutServerLoad } from './$types';

// Settings are for the people who run the map, never for a watcher following a link.
//
// The map is loaded once for the whole section. The per-viewer settings are not: they hang
// off their own key on the pages that use them, so a toggle does not refetch the chain.
export const load: LayoutServerLoad = async (event) => {
	// Started before waiting on the layout above, so the two run side by side. A signed-out
	// visitor's request is simply refused and never looked at.
	const viewing = mapView(event, mapIdFromSlug(event.params.id)).catch((err: unknown) =>
		err instanceof Error ? err : new Error(String(err)),
	);
	const { me, offline: down } = await event.parent();
	if (down) offline();
	if (!me) redirect(302, `/login?return_to=${encodeURIComponent(event.url.pathname)}`);

	const view = await viewing;
	if (view instanceof Error) mapLoadFailed(view, true);
	// A bare id, or the name from before a rename, settles on the current address, as the
	// map page does. The section links are built from it, so on any other address none of
	// them would read as the one you are on.
	const slug = mapSlug(view.map);
	if (event.params.id !== slug) {
		const rest = event.url.pathname.slice(`/maps/${event.params.id}`.length);
		redirect(302, `/maps/${slug}${rest}${event.url.search}`);
	}
	return { view };
};
