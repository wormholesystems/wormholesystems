<script lang="ts" module>
	/** Named as values as well as a type, so what is stored can be checked against them. */
	export const SORT_COLUMNS = ['id', 'category', 'type', 'age'] as const;
	export type SortColumn = (typeof SORT_COLUMNS)[number];
</script>

<script lang="ts">
	// The signature list's column header. The widths here are the row's widths, so the two
	// have to stay together; keeping them in one component is what makes that true.
	import SortHeader from './SortHeader.svelte';

	let {
		compact = false,
		sort,
		onsort,
	}: {
		compact?: boolean;
		sort?: { column: SortColumn; direction: 'asc' | 'desc' };
		/** Omitted where the header is not sortable, which leaves the arrows off. */
		onsort?: (column: SortColumn) => void;
	} = $props();
</script>

<div
	class="flex items-center gap-2 border-b border-border/30 bg-muted/20 px-3 font-mono text-[10px] tracking-wider text-muted-foreground uppercase {compact
		? 'py-0.5'
		: 'py-1.5'}"
>
	<SortHeader column="id" {sort} {onsort} class="w-16 shrink-0">ID</SortHeader>
	<SortHeader column="category" {sort} {onsort} class="w-26 shrink-0">Cat</SortHeader>
	<SortHeader column="type" {sort} {onsort} class="min-w-0 flex-1">Type</SortHeader>
	<span class="min-w-0 flex-1">Conn</span>
	<SortHeader column="age" {sort} {onsort} class="w-10 shrink-0 justify-end">Age</SortHeader>
	<span class="w-12 shrink-0"></span>
</div>
