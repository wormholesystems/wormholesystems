<script lang="ts">
	// The map's tabs, in the status bar beside its name. Each is a map of its own to work
	// on, with its own systems, connections and history; routes and what is known about a
	// system cross between them. A map with a single tab shows none at all, unless the
	// viewer is somebody who could add a second.
	import ArrowLeftIcon from '@lucide/svelte/icons/arrow-left';
	import ArrowRightIcon from '@lucide/svelte/icons/arrow-right';
	import EllipsisIcon from '@lucide/svelte/icons/ellipsis';
	import PencilIcon from '@lucide/svelte/icons/pencil';
	import PlusIcon from '@lucide/svelte/icons/plus';
	import Trash2Icon from '@lucide/svelte/icons/trash-2';

	import type { MapTab } from '$lib/api/types/MapTab';
	import Hint from '$lib/components/Hint.svelte';
	import * as DropdownMenu from '$lib/components/ui/dropdown-menu';
	import { confirmDanger } from '$lib/confirm.svelte';
	import type { MapState } from '../../state/map-state.svelte';

	let { map }: { map: MapState } = $props();

	/** The server's limits, mirrored so the bar can stop offering what would be refused. */
	const MAX_TABS = 12;
	const MAX_NAME = 24;

	const tabs = $derived(map.tabs.all);
	const canManage = $derived(map.canManage);
	// The operator's switch: off, no tab can be added, and a map nobody has split shows
	// no bar at all.
	const canAdd = $derived(canManage && map.tabsEnabled);
	const visible = $derived(tabs.length > 1 || canAdd);

	// The name being typed: for a new tab, or for the one being renamed.
	let naming = $state<{ tabId: number | null; value: string } | null>(null);

	function focusName(el: HTMLInputElement) {
		el.focus();
		el.select();
	}

	function commitName() {
		const pending = naming;
		naming = null;
		const name = pending?.value.trim() ?? '';
		if (!pending || !name) return;
		if (pending.tabId === null) {
			map.tabs.create(name);
			return;
		}
		const tab = tabs.find((t) => t.id === pending.tabId);
		if (tab && name !== tab.name) map.tabs.rename(tab, name);
	}

	async function remove(tab: MapTab) {
		const sure = await confirmDanger({
			title: `Delete the tab ${tab.name}?`,
			body: 'Everything drawn on it goes with it: its systems, connections, signatures and history. The other tabs are not touched. This cannot be undone.',
			action: 'Delete tab',
		});
		if (sure) map.tabs.remove(tab);
	}
</script>

{#snippet nameInput()}
	<input
		class="h-8 w-32 rounded-t-md bg-background px-2 text-xs text-foreground outline-none ring-1 ring-ring ring-inset"
		maxlength={MAX_NAME}
		placeholder="Tab name"
		aria-label="Tab name"
		data-testid="tab-name-input"
		value={naming?.value ?? ''}
		oninput={(ev) => {
			if (naming) naming = { ...naming, value: ev.currentTarget.value };
		}}
		onkeydown={(ev) => {
			// The canvas listens for Delete and Backspace to remove the selection.
			ev.stopPropagation();
			if (ev.key === 'Enter') commitName();
			if (ev.key === 'Escape') naming = null;
		}}
		onblur={commitName}
		use:focusName
	/>
{/snippet}

{#if visible}
	<!-- Sat on the bar's bottom edge and pulled a pixel over it, so the tab in view covers
	     the hairline under it and reads as one surface with the map below. -->
	<div
		class="-mb-px ml-2 flex min-w-0 items-end gap-0.5 self-end overflow-x-auto text-xs"
		role="group"
		aria-label="Map tabs"
		data-testid="tab-bar"
	>
		{#each tabs as tab, index (tab.id)}
			{@const active = tab.id === map.mapId}
			<div
				class="flex h-8 shrink-0 items-center rounded-t-md border border-b-0 {active
					? 'border-border bg-background font-medium text-foreground'
					: 'border-transparent text-muted-foreground hover:bg-accent/50 hover:text-foreground'}"
			>
				{#if naming?.tabId === tab.id}
					{@render nameInput()}
				{:else}
					<button
						class="h-full max-w-40 truncate px-3"
						aria-pressed={active}
						data-testid="tab"
						data-tab-id={tab.id}
						onpointerenter={() => map.tabs.warm(tab)}
						onfocus={() => map.tabs.warm(tab)}
						onclick={() => map.tabs.open(tab)}
						ondblclick={() => {
							if (canManage) naming = { tabId: tab.id, value: tab.name };
						}}
					>
						{tab.name}
					</button>
					{#if canManage && active}
						<DropdownMenu.Root>
							<DropdownMenu.Trigger>
								{#snippet child({ props })}
									<button
										{...props}
										class="-ml-1 flex h-full items-center pr-2 text-muted-foreground hover:text-foreground"
										aria-label="Tab options"
										data-testid="tab-menu"
									>
										<EllipsisIcon class="size-3.5" />
									</button>
								{/snippet}
							</DropdownMenu.Trigger>
							<DropdownMenu.Content class="w-44" align="start">
								<DropdownMenu.Item onSelect={() => (naming = { tabId: tab.id, value: tab.name })}>
									<PencilIcon class="size-4" />
									Rename
								</DropdownMenu.Item>
								<DropdownMenu.Item disabled={index === 0} onSelect={() => map.tabs.move(tab, -1)}>
									<ArrowLeftIcon class="size-4" />
									Move left
								</DropdownMenu.Item>
								<DropdownMenu.Item
									disabled={index === tabs.length - 1}
									onSelect={() => map.tabs.move(tab, 1)}
								>
									<ArrowRightIcon class="size-4" />
									Move right
								</DropdownMenu.Item>
								<!-- The first tab is the map itself and goes only with it. -->
								{#if !tab.is_root}
									<DropdownMenu.Separator />
									<DropdownMenu.Item
										variant="destructive"
										onSelect={() => remove(tab)}
										data-testid="tab-delete"
									>
										<Trash2Icon class="size-4" />
										Delete tab
									</DropdownMenu.Item>
								{/if}
							</DropdownMenu.Content>
						</DropdownMenu.Root>
					{/if}
				{/if}
			</div>
		{/each}

		{#if canAdd && naming?.tabId === null}
			{@render nameInput()}
		{:else if canAdd && tabs.length < MAX_TABS}
			<Hint text="Add a tab" label>
				{#snippet child({ props })}
					<button
						{...props}
						class="mb-1 flex size-6 shrink-0 items-center justify-center rounded text-muted-foreground hover:bg-accent hover:text-foreground"
						data-testid="tab-add"
						onclick={() => (naming = { tabId: null, value: '' })}
					>
						<PlusIcon class="size-3.5" />
					</button>
				{/snippet}
			</Hint>
		{/if}
	</div>
{/if}
