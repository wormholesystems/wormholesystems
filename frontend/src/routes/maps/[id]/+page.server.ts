import { redirect } from '@sveltejs/kit';

import { mapIdFromSlug, mapSlug } from '$lib/map/slug';
import { tabFromUrl } from '$lib/map/tabs';
import { gridConfig, mapSignatures, mapUserSettings, mapView } from '$lib/server/api';
import { mapLoadFailed } from '$lib/server/load-errors';
import type { PageServerLoad } from './$types';

// The graph and the viewer's arrangement, fetched with the page rather than after it, so
// the first frame the page paints is the map rather than a placeholder. The signatures and
// the grid ride along for the same reason: the signature panel paints with the map instead
// of a beat later, and the canvas is never drawn on the built-in grid first. `MapState`
// starts from all of it and takes over.
//
// The graph is the one part the page cannot do without: a map that is gone, was never
// shared with this visitor, or a backend that is down ends on the error page that says
// which, with the status that goes with it. The rest only fall back to the client.
export const load: PageServerLoad = async (event) => {
	const root = mapIdFromSlug(event.params.id);
	// `?tab=` names the tab in view. A tab is a map of its own underneath, so it is simply
	// the id everything below is read under; without one, that is the map itself.
	const tab = tabFromUrl(event.url) ?? root;
	const id = tab;
	const [view, settings, signatures, grid] = await Promise.all([
		mapView(event, id).catch((err: unknown) =>
			err instanceof Error ? err : new Error(String(err)),
		),
		mapUserSettings(event, id).catch(() => null),
		mapSignatures(event, id).catch(() => null),
		gridConfig(event).catch(() => null),
	]);
	// A tab that has been deleted, or one of another map, is not an error page: the link
	// still names a map, and that is where it lands.
	if (tab !== root && (view instanceof Error || view.root_id !== root)) {
		const url = new URL(event.url);
		url.searchParams.delete('tab');
		redirect(302, `${url.pathname}${url.search}`);
	}
	if (view instanceof Error || !Number.isSafeInteger(id)) {
		const { me } = await event.parent();
		mapLoadFailed(view, me !== null);
	}
	// A bare id, or the name from before a rename, settles on the current address. Not
	// permanent: a map renamed back would otherwise bounce between two cached redirects.
	// The address is the map's, whichever of its tabs is in view.
	const slug = mapSlug({ id: view.root_id, name: view.map.name });
	if (event.params.id !== slug) redirect(302, `/maps/${slug}${event.url.search}`);
	// A map link is the one people actually paste at each other, so it says which map.
	return { view, settings, signatures, grid, seo: { title: view.map.name } };
};
