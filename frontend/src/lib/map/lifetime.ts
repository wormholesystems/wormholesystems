// When a wormhole is expected to die, from what the map knows about it.

import type { MapConnection } from '$lib/api/types/MapConnection';

const HOUR_MS = 3_600_000;

/**
 * EOL means under four hours left and critical under one: a mark starts that countdown.
 * Expired promises nothing at all, so it is due the moment it is marked.
 */
const LEFT_AFTER_MARK_MS = { eol: 4 * HOUR_MS, critical: HOUR_MS, expired: 0 } as const;

/**
 * The server sends every hole's lifetime from its class pair. Should it be missing, the
 * ordinary day stands in, which is also what the server uses for a shape it cannot read.
 */
const FALLBACK_LIFETIME_HOURS = 24;

export interface LifetimeDeadline {
	/** Epoch milliseconds. */
	at: number;
	/**
	 * Before any mark the clock runs from when the hole was mapped, which is at best when it
	 * opened, so the deadline is a ceiling rather than a time.
	 */
	estimated: boolean;
}

/**
 * When the hole should be gone, or null for a stargate. Follows the background ageing: a
 * mark is timed from when it was made, since someone read it off the hole itself, and
 * anything else from the hole's age against the lifetime its class pair gives it.
 */
export function lifetimeDeadline(
	c: Pick<
		MapConnection,
		'kind' | 'time_status' | 'time_status_updated_at' | 'created_at' | 'lifetime_hours'
	>,
): LifetimeDeadline | null {
	if (c.kind !== 'wormhole') return null;
	if (c.time_status && c.time_status !== 'stable' && c.time_status_updated_at) {
		return {
			at: Date.parse(c.time_status_updated_at) + LEFT_AFTER_MARK_MS[c.time_status],
			estimated: false,
		};
	}
	const hours = c.lifetime_hours ?? FALLBACK_LIFETIME_HOURS;
	return { at: Date.parse(c.created_at) + hours * HOUR_MS, estimated: true };
}

/**
 * Only the largest unit, rounded down: "2h", "45m", "<1m", and "0m" once it has run out. A
 * label read at a glance says as much with "1h" as with "1h 32m". Hours at most, since a
 * hole's life is counted in hours even when it has two days of it.
 */
export function formatRemaining(deadlineMs: number, nowMs: number): string {
	const left = deadlineMs - nowMs;
	if (left <= 0) return '0m';
	const minutes = Math.floor(left / 60_000);
	if (minutes < 1) return '<1m';
	return minutes < 60 ? `${minutes}m` : `${Math.floor(minutes / 60)}h`;
}

/**
 * The same for a readout with room to talk: a hole past its deadline is closing any moment
 * rather than sitting at zero.
 */
export function describeRemaining(deadlineMs: number, nowMs: number): string {
	return deadlineMs <= nowMs ? 'any moment now' : formatRemaining(deadlineMs, nowMs);
}
