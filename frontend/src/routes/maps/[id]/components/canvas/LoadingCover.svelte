<script lang="ts">
	// A cover that appears and vanishes inside a few frames reads as a flicker, so it stays
	// for a moment even when the map was quick. It is only ever a floor, never a delay: a
	// slow map keeps it until the data is in.
	import { fade } from 'svelte/transition';

	import { cn } from '$lib/utils';
	import type { MapState } from '../../state/map-state.svelte';

	let { map, top }: { map: MapState; top: number } = $props();

	const COVER_MS = 250;
	// Once per screen: switching maps builds a new one, and the gap before it is ready
	// should look like loading rather than like nothing. Another tab of the same map is
	// the same screen, already up, so it is never covered.
	let covered = $state(true);
	$effect(() => {
		const timer = setTimeout(() => (covered = false), COVER_MS);
		return () => clearTimeout(timer);
	});
	const revealed = $derived(map.ready && !covered);
</script>

{#if !revealed}
	<!-- Click-through once the map is really ready: what is left is the fade, and a
	     cover that swallows the first click of the session would be worse than no cover. -->
	<div
		class={cn(
			// Above the dialog layer: the introduction belongs to the map, so it waits for it.
			'fixed inset-x-0 bottom-0 z-60 overflow-hidden bg-card',
			'flex items-center justify-center',
			map.ready && 'pointer-events-none',
		)}
		style:top="{top}px"
		data-testid="map-loading"
		out:fade={{ duration: 200 }}
	>
		<div class="flex flex-col items-center gap-5">
			<svg class="size-9 animate-spin text-muted-foreground" viewBox="0 0 36 36" fill="none">
				<circle
					cx="18"
					cy="18"
					r="16"
					stroke="currentColor"
					stroke-opacity="0.15"
					stroke-width="1.5"
				/>
				<path
					d="M34 18A16 16 0 0 0 18 2"
					stroke="currentColor"
					stroke-width="1.5"
					stroke-linecap="round"
				/>
			</svg>
			<div class="flex flex-col items-center gap-1.5">
				<p class="font-mono text-[10px] tracking-[0.35em] text-muted-foreground uppercase">
					Loading
				</p>
				<p class="text-sm font-medium">{map.data?.map.name ?? ''}</p>
			</div>
		</div>
	</div>
{/if}
