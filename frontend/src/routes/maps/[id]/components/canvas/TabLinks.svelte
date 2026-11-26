<script lang="ts">
	// The other tabs that draw this node's system. Tabs are worked on separately and know
	// nothing of each other's signatures or names, so this is all one says about another:
	// the same system is on both, and a click goes to it there.
	import CornerDownRightIcon from '@lucide/svelte/icons/corner-down-right';

	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { NODE_W, type Vec2 } from '$lib/map/geometry';
	import type { MapTab } from '$lib/api/types/MapTab';
	import type { MapState } from '../../state/map-state.svelte';

	let {
		map,
		tabs,
		solarSystemId,
		pos,
	}: { map: MapState; tabs: MapTab[]; solarSystemId: number; pos: Vec2 } = $props();

	const CHIP =
		'flex h-4 max-w-full items-center gap-1 rounded-sm border border-border bg-card px-1 text-[10px] leading-none text-muted-foreground hover:border-foreground/40 hover:text-foreground';
</script>

<!-- In the gap under the node, which the placement helper keeps clear. The press is
     stopped, or the canvas would start a rubber band under the click. -->
<!-- svelte-ignore a11y_no_static_element_interactions -->
<div
	class="absolute flex"
	style:left="{pos.x}px"
	style:top="{pos.y + map.nodeH + 2}px"
	style:width="{NODE_W}px"
	data-testid="tab-links"
	onpointerdown={(ev) => ev.stopPropagation()}
	oncontextmenu={(ev) => ev.stopPropagation()}
>
	{#if tabs.length === 1}
		<button
			class={CHIP}
			title={`Also on the tab ${tabs[0].name}`}
			data-testid="tab-link"
			onclick={() => map.tabs.open(tabs[0], solarSystemId)}
		>
			<CornerDownRightIcon class="size-3 shrink-0" />
			<span class="truncate">{tabs[0].name}</span>
		</button>
	{:else}
		<DropdownMenu.Root>
			<DropdownMenu.Trigger>
				{#snippet child({ props })}
					<button {...props} class={CHIP} data-testid="tab-link">
						<CornerDownRightIcon class="size-3 shrink-0" />
						<span class="truncate">{tabs.length} other tabs</span>
					</button>
				{/snippet}
			</DropdownMenu.Trigger>
			<DropdownMenu.Content class="w-44" align="start">
				{#each tabs as tab (tab.id)}
					<DropdownMenu.Item onSelect={() => map.tabs.open(tab, solarSystemId)}>
						<span class="truncate">{tab.name}</span>
					</DropdownMenu.Item>
				{/each}
			</DropdownMenu.Content>
		</DropdownMenu.Root>
	{/if}
</div>
