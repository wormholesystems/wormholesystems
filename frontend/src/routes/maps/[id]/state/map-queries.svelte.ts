// The map page's server data, as queries. Socket frames become invalidations here, and
// the mutations carry the policy every map write shares. Component init only: the
// queries need the provider's client.

import {
	createMutation,
	createQuery,
	keepPreviousData,
	useQueryClient,
} from '@tanstack/svelte-query';
import { untrack } from 'svelte';
import { toast } from 'svelte-sonner';

import { errorMessage } from '$lib/api/client';
import { key, q } from '$lib/api/queries';
import type { MapEvent } from '$lib/api/types/MapEvent';
import type { MapUserSettings } from '$lib/api/types/MapUserSettings';
import type { MapView } from '$lib/api/types/MapView';
import type { GridConfig } from '$lib/api/types/GridConfig';
import type { Signature } from '$lib/api/types/Signature';
import { MAP_ACTIONS, type MapAction } from '$lib/map/actions';
import { atLeast } from '$lib/map/roles';
import { aggressorLabel, eventMeta, eventSentence, fresh } from '$lib/map/structures';
import { createBackgroundImageWriter, createUserSettingsWriter } from '$lib/map/user-settings';
import { createCoalescer } from './coalesce';
import { keysFor } from './invalidations';

/** One write through the shared mutation: what to run, and which action's copy to speak. */
interface MapWrite {
	action: MapAction;
	exec: () => Promise<unknown>;
	detail?: string;
}

/** What the page's load already fetched, handed over so the first frame has it. */
export interface MapSeed {
	view: MapView | null;
	settings: MapUserSettings | null;
	signatures?: Signature[] | null;
	grid?: GridConfig | null;
}

/** Long enough to swallow a paste-scan burst, short enough that nobody notices. */
const BURST_MS = 60;

export function createMapQueries(
	/** The tab in view. Read on every use: the screen stays up while it changes. */
	mapId: () => number,
	signedIn: boolean,
	seed: MapSeed,
	/** Whether the map socket is open, and the server echo will drive the refetch. */
	isLive: () => boolean,
) {
	const client = useQueryClient();

	// Seeded via setQueryData rather than initialData: initialData is ignored whenever the
	// cache entry already exists (revisiting a map), which would silently discard the
	// fresh payload this navigation's load just fetched. Stamped fresh-now, so staleTime
	// suppresses the mount refetch, while a socket-driven invalidation always refetches.
	// Only a seed that was loaded for this very tab: one fetched for the tab just left
	// would otherwise be filed under the new tab's keys as fresh.
	const plant = (id: number, seed: MapSeed) => {
		const mine = seed.view?.map.id === id;
		if (seed.view && mine) client.setQueryData(key.mapView(id), seed.view);
		if (seed.settings) client.setQueryData(key.userSettings(id), seed.settings);
		if (seed.signatures && mine) client.setQueryData(key.signatures(id), seed.signatures);
		// The grid is the same for every map and never goes stale, so a copy already in the
		// cache stays; the seed only fills it on the first map of the session.
		if (seed.grid && client.getQueryData(q.gridConfig().queryKey) === undefined) {
			client.setQueryData(q.gridConfig().queryKey, seed.grid);
		}
	};
	plant(untrack(mapId), seed);

	// What belongs to the map rather than to one tab of it is the same under every tab's
	// id, so going to a tab not visited yet keeps showing it while that tab's copy loads.
	const mapWide = { placeholderData: keepPreviousData };

	// The graph is planted before a tab is shown, so the placeholder is only the net under
	// a navigation that brought none: the screen stays up rather than going back to loading.
	const graph = createQuery(() => ({
		...q.mapView(mapId()),
		...mapWide,
		meta: { toastOnError: true },
	}));
	const signatures = createQuery(() => ({
		...q.listSignatures(mapId()),
		meta: { toastOnError: true },
	}));
	const watchlist = createQuery(() => ({
		...q.listWatchlist(mapId()),
		...mapWide,
		meta: { toastOnError: true },
	}));
	// Signed-in only: a watcher on a share link has no list of their own.
	const personalWatchlist = createQuery(() => ({
		...q.listPersonalWatchlist(mapId()),
		...mapWide,
		enabled: signedIn,
	}));
	const ignored = createQuery(() => ({
		...q.listIgnored(mapId()),
		...mapWide,
		meta: { toastOnError: true },
	}));
	// Everything below fails silently, as the old fetchers did: a watcher's 403 or a blip
	// leaves the panel empty rather than raising a toast.
	const history = createQuery(() => ({ ...q.mapHistory(mapId()), enabled: signedIn }));
	const stale = createQuery(() => ({ ...q.listStaleConnections(mapId()), enabled: signedIn }));
	const settings = createQuery(() => ({
		...q.mapUserSettings(mapId()),
		...mapWide,
		enabled: signedIn,
	}));
	const characters = createQuery(() => ({
		...q.mapCharacters(mapId()),
		...mapWide,
		enabled: signedIn,
		// Movement arrives over the sockets; this is only the net under a dropped frame.
		refetchInterval: 120_000,
		refetchIntervalInBackground: true,
	}));
	// What members share of their structures is for members: a viewer is not asked, since
	// the answer would only be a refusal.
	const structures = createQuery(() => ({
		...q.mapStructures(mapId()),
		...mapWide,
		enabled: signedIn && atLeast(graph.data?.role, 'member'),
	}));
	// An attack is worth interrupting for whether or not the card is on screen. The first
	// load is what was already there; after it, anything new that only just happened.
	let seenEvents: Set<number> | null = null;
	$effect(() => {
		const events = structures.data?.events;
		if (!events) return;
		for (const event of fresh(events, seenEvents, new Date())) {
			const notify = { attack: toast.error, fuel: toast.warning, routine: toast.info }[
				eventMeta(event.kind).tone
			];
			notify(eventSentence(event), {
				description:
					[event.system_name, aggressorLabel(event)].filter(Boolean).join(' · ') || undefined,
				duration: 15_000,
			});
		}
		seenEvents = new Set(events.map((event) => event.id));
	});
	const grid = createQuery(() => q.gridConfig());
	const eveScout = createQuery(() => q.eveScout());
	// The focus trigger lives in the map session (a window listener), not here: the jump
	// tracker wants window focus, and the query's own option watches visibilitychange.
	const myCharacters = createQuery(() => ({
		...q.myCharacters(),
		enabled: signedIn,
		// Movement arrives over the sockets; the interval is the net under a dropped frame.
		refetchInterval: 120_000,
		refetchIntervalInBackground: true,
	}));

	// The cache itself serialises the refetches, and an aborted fetch never lands, so
	// coalescing is all that remains of the old scheduler.
	const coalescer = createCoalescer((keys) => {
		for (const queryKey of keys) void client.invalidateQueries({ queryKey });
	}, BURST_MS);

	const invalidateAll = () => client.invalidateQueries({ queryKey: key.map(mapId()) });

	const write = createMutation(() => ({
		mutationFn: (vars: MapWrite) => vars.exec(),
		onSuccess: (_data: unknown, vars: MapWrite) => {
			const copy = MAP_ACTIONS[vars.action];
			if ('done' in copy && copy.done) toast.success(copy.done, { description: vars.detail });
			// While the socket is open the server echoes the change back and `applyEvent`
			// refetches exactly the part that moved; refetching here as well would make the
			// person doing the editing pay twice for every write. No echo without a socket,
			// so the fallback stands in.
			if (!isLive()) void invalidateAll();
		},
		onError: (err: unknown, vars: MapWrite) =>
			toast.error(MAP_ACTIONS[vars.action].failed, { description: errorMessage(err) }),
	}));

	const settingsWriter = createUserSettingsWriter();
	const backgroundWriter = createBackgroundImageWriter();

	return {
		client,
		graph,
		signatures,
		watchlist,
		personalWatchlist,
		ignored,
		history,
		stale,
		settings,
		characters,
		grid,
		eveScout,
		structures,
		myCharacters,
		write,
		settingsWriter,
		backgroundWriter,
		invalidateAll,
		/** File what a navigation loaded for a tab, before the screen is shown that tab. */
		plant,
		/** Refetch what one socket frame invalidated; [`keysFor`] holds the table. */
		applyEvent(event: MapEvent | null) {
			for (const k of keysFor(mapId(), event)) coalescer.schedule(k);
		},
		/** An optimistic local edit of the viewer's settings, without a round trip. */
		patchSettingsLocal(update: (s: MapUserSettings) => MapUserSettings) {
			client.setQueryData(key.userSettings(mapId()), (s: MapUserSettings | undefined) =>
				s ? update(s) : s,
			);
		},
	};
}

export type MapQueries = ReturnType<typeof createMapQueries>;
