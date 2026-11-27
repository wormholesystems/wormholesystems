<script lang="ts">
	// The desktop app's overlay, redrawn: it is a native window over the game, so there is
	// no page of the website to take a screenshot of. It shows the signature demo's system
	// right after a paste and a jump, on invented data, and is dark in either theme because
	// what it floats over is. Change it with the panel in `desktop/src/sigs.rs`.
	import CopyIcon from '@lucide/svelte/icons/copy';
	import EllipsisVerticalIcon from '@lucide/svelte/icons/ellipsis-vertical';
	import MapIcon from '@lucide/svelte/icons/map';
	import SettingsIcon from '@lucide/svelte/icons/settings';
	import UserIcon from '@lucide/svelte/icons/user';
	import XIcon from '@lucide/svelte/icons/x';

	type Row = {
		id: string;
		category?: string;
		type?: string;
		/** The far end's class, its color, the system and its region. */
		far?: [string, string, string, string];
		age: string;
		mark?: 'new' | 'gone';
	};

	const rows: Row[] = [
		{
			id: 'ABC-123',
			category: 'WH',
			type: 'K162 C5',
			far: ['C5', 'text-red-400', 'J123746', 'E-R00028'],
			age: '3h',
		},
		{
			id: 'DEF-456',
			category: 'WH',
			type: 'N944 L',
			far: ['L', 'text-orange-400', 'Korasen', 'Black Rise'],
			age: 'now',
		},
		{ id: 'GHI-789', category: 'WH', age: '3h' },
		{ id: 'JKL-012', category: 'Relic', type: 'Frontier Enclave', age: '3h', mark: 'gone' },
		{ id: 'MNO-345', category: 'Combat', type: 'Core Garrison', age: '3h' },
		{ id: 'PQR-678', age: 'now', mark: 'new' },
		{ id: 'STU-901', category: 'Gas', age: 'now', mark: 'new' },
	];

	// Narrow, it drops the category and the age, as the overlay itself does.
	const columns =
		'grid-cols-[4rem_minmax(0,5.5rem)_minmax(0,1fr)_1.5rem] sm:grid-cols-[4rem_3.5rem_minmax(0,6rem)_minmax(0,1fr)_2rem_2.5rem]';

	const keys = [
		['Ctrl+Alt+V', 'paste the scan'],
		['Ctrl+Alt+D', 'delete what is gone'],
		['Ctrl+Alt+1', 'the hole you jumped'],
		['Ctrl+Alt+C', 'mass critical'],
	];
</script>

<figure
	class="stage relative isolate overflow-hidden border border-border shadow-2xl shadow-black/10 dark:shadow-black/50"
	data-testid="overlay-demo"
	aria-label="The desktop app's overlay over the game: Turnur's seven signatures, two of them new and one gone after a paste, and a note that the jump to Korasen was mapped and its bookmark copied."
>
	<div class="flex flex-col gap-6 p-4 sm:p-8 md:flex-row md:items-end md:justify-between">
		<ul class="order-last flex flex-col gap-2 md:order-first" aria-hidden="true">
			{#each keys as [key, what] (key)}
				<li class="flex items-center gap-2.5 text-xs text-neutral-400">
					<kbd
						class="border border-white/15 bg-white/5 px-1.5 py-0.5 font-mono text-[10px] text-neutral-200"
						>{key}</kbd
					>
					{what}
				</li>
			{/each}
		</ul>

		<div
			class="relative w-full max-w-[460px] border border-white/12 bg-neutral-950/85 text-neutral-100 backdrop-blur md:ml-auto"
			aria-hidden="true"
		>
			<span class="absolute inset-x-0 -top-px h-px bg-gradient-to-r from-sky-400/70 to-sky-400/10"
			></span>
			<div class="flex flex-col gap-0.5 border-b border-white/12 px-2 pt-1.5 pb-2">
				<div class="flex h-[22px] items-center gap-1.5 text-sm font-semibold">
					<span class="text-orange-400">L</span>
					<span>T</span>
					<span class="min-w-0 flex-1 truncate font-normal text-neutral-400">Turnur</span>
					<SettingsIcon class="size-3.5 text-neutral-400" />
					<XIcon class="ml-1.5 size-3.5 text-neutral-400" />
				</div>
				<div class="flex items-center gap-2 text-xs text-neutral-400">
					<UserIcon class="size-3 shrink-0 opacity-70" />
					<span class="truncate">Your scout</span>
					<MapIcon class="size-3 shrink-0 opacity-70" />
					<span class="truncate">Home Chain / Turnur</span>
					<span class="ml-auto shrink-0 font-mono">7 sigs</span>
				</div>
			</div>
			<div
				class="grid h-[22px] {columns} items-center gap-x-2 border-b border-white/6 px-2 text-[11px] text-neutral-400"
			>
				<span class="text-neutral-100">ID</span>
				<span class="max-sm:hidden">Category</span>
				<span>Type</span>
				<span>Connection</span>
				<span class="text-right max-sm:hidden">Age</span>
			</div>
			<!-- Room under the rows for the note, which lies over the list rather than in it. -->
			<div class="relative pb-[26px]">
				{#each rows as row (row.id)}
					<div
						class="grid h-[26px] {columns} items-center gap-x-2 border-b border-white/6 px-2 text-xs {row.mark ===
						'new'
							? 'bg-green-500/10'
							: row.mark === 'gone'
								? 'bg-red-500/10'
								: ''}"
					>
						<span
							class="font-mono {row.mark === 'new'
								? 'text-green-400'
								: row.mark === 'gone'
									? 'text-red-400'
									: ''}">{row.id}</span
						>
						<span class="max-sm:hidden {row.category ? '' : 'text-neutral-500'}"
							>{row.category ?? 'Category'}</span
						>
						<span class="truncate {row.type ? '' : 'text-neutral-500'}">{row.type ?? 'Type'}</span>
						<span class="flex min-w-0 items-center gap-1.5 overflow-hidden">
							{#if row.far}
								<span class={row.far[1]}>{row.far[0]}</span>
								<span class="shrink-0">{row.far[2]}</span>
								<span class="truncate text-neutral-500">{row.far[3]}</span>
							{/if}
						</span>
						<span class="text-right text-neutral-400 max-sm:hidden">{row.age}</span>
						<span class="flex items-center justify-end gap-1.5 text-neutral-400">
							{#if row.far}<CopyIcon class="size-3 max-sm:hidden" />{/if}
							<EllipsisVerticalIcon class="size-3" />
						</span>
					</div>
				{/each}
				<p
					class="absolute inset-x-0 bottom-0 flex h-[26px] items-center gap-2 border-t border-green-500/40 bg-neutral-950 px-2 text-xs text-green-400"
				>
					<span class="size-1.5 rounded-full bg-green-400"></span>
					Jumped to Korasen. Bookmark copied.
				</p>
			</div>
			<div class="flex items-center gap-2 px-2 py-2">
				<span class="border border-sky-400/50 bg-sky-400/10 px-2.5 py-1 text-xs">Paste scan</span>
				<span class="border border-red-400/40 px-2.5 py-1 text-xs text-red-300">
					Delete 1 missing
				</span>
			</div>
		</div>
	</div>
</figure>

<style>
	/* Space behind it: dark in either theme, with a little light where a sun might be. */
	.stage {
		background:
			radial-gradient(ellipse 60% 70% at 18% 20%, rgb(56 189 248 / 0.1), transparent 70%),
			radial-gradient(ellipse 50% 60% at 85% 90%, rgb(245 158 11 / 0.08), transparent 70%), #07080b;
	}
</style>
