import type { MapConnection } from '$lib/api/types/MapConnection';
import { api } from '$lib/api/client';
import type { BookmarkName } from '$lib/api/types/BookmarkName';
import type { NameBookmark } from '$lib/api/types/NameBookmark';
import type { MapSystemView } from '$lib/api/types/MapSystemView';
import type { Signature } from '$lib/api/types/Signature';
import type { UpdateSignature } from '$lib/api/types/UpdateSignature';

/**
 * The fields a row may change. Taken from the wire type rather than restated, so a field
 * that changes shape on the server stops compiling here; an open dictionary would have
 * accepted a misspelled key and quietly changed nothing.
 */
export type SignaturePatch = Omit<UpdateSignature, 'map_id' | 'signature_pk'>;

/**
 * What the signature row and its inputs need from the map around them: the placements and
 * edges they resolve names against, and the writes they can make.
 *
 * They used to take the whole `MapState`, which meant they could only ever be rendered on
 * a live map. Naming the surface here keeps every write in one place, lets the row be
 * tested against plain arrays, and lets the landing page render the real component from
 * static data instead of a look-alike that drifts.
 */
export interface SignatureContext {
	systems: MapSystemView[];
	connections: MapConnection[];
	/** The same placements and edges by id, built once, since every row looks its own up. */
	systemById: ReadonlyMap<number, MapSystemView>;
	connectionById: ReadonlyMap<number, MapConnection>;
	sigs: Signature[];
	/**
	 * The bookmark name the map's naming strategy gives a hole, for a row's copy button.
	 * Absent where there is no map on the server to ask, and then there is no button.
	 */
	nameBookmark?: (cmd: NameBookmark) => Promise<BookmarkName>;
	/** Absent in a read-only rendering; with no actions nothing can be written. */
	actions?: SignatureActions;
}

export interface SignatureActions {
	update(signaturePk: number, patch: SignaturePatch): void;
	remove(signaturePk: number): void;
	link(signaturePk: number, connectionId: number): void;
	unlink(signaturePk: number): void;
	setPreserveMass(connectionId: number, preserve: boolean): void;
}

/** The map's domain namespaces, narrowed to what the signature rows use. */
interface SignatureHost {
	readonly mapId: number;
	systems: {
		readonly all: MapSystemView[];
		readonly byId: ReadonlyMap<number, MapSystemView>;
	};
	connections: {
		readonly all: MapConnection[];
		readonly byId: ReadonlyMap<number, MapConnection>;
		patch(connectionId: number, patch: { preserve_mass: boolean }): void;
	};
	signatures: { readonly all: Signature[] } & Omit<SignatureActions, 'setPreserveMass'>;
}

/**
 * The live context over a map. The rows and their inputs take this rather than the whole
 * map, so the same components render from static data elsewhere; every write they can
 * make is named here.
 */
export function makeSignatureContext(map: SignatureHost): SignatureContext {
	return {
		get systems() {
			return map.systems.all;
		},
		get connections() {
			return map.connections.all;
		},
		get systemById() {
			return map.systems.byId;
		},
		get connectionById() {
			return map.connections.byId;
		},
		get sigs() {
			return map.signatures.all;
		},
		nameBookmark: (cmd) => api.nameBookmark(map.mapId, cmd),
		actions: {
			update: (signaturePk, patch) => map.signatures.update(signaturePk, patch),
			remove: (signaturePk) => map.signatures.remove(signaturePk),
			link: (signaturePk, connectionId) => map.signatures.link(signaturePk, connectionId),
			unlink: (signaturePk) => map.signatures.unlink(signaturePk),
			setPreserveMass: (connectionId, preserve) =>
				map.connections.patch(connectionId, { preserve_mass: preserve }),
		},
	};
}
