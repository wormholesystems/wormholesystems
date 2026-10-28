<script lang="ts">
	// The alias sequence and the three bookmark formats. The formats are opaque token strings,
	// so each previews against a worked example as you type, rendered by the server the same
	// way it names everything else.
	import { createQuery, keepPreviousData } from '@tanstack/svelte-query';

	import { oneOf } from '$lib/lookup';

	import { api } from '$lib/api/client';
	import { debounced } from '$lib/debounced.svelte';
	import { draft as draftOf } from '$lib/draft.svelte';
	import type { AliasScheme } from '$lib/api/types/AliasScheme';
	import type { MapNaming } from '$lib/api/types/MapNaming';
	import type { NamingStrategyKind } from '$lib/api/types/NamingStrategyKind';
	import { Button } from '$lib/components/ui/button';
	import * as Card from '$lib/components/ui/card';
	import * as Field from '$lib/components/ui/field';
	import { Input } from '$lib/components/ui/input';
	import * as ToggleGroup from '$lib/components/ui/toggle-group';

	let {
		naming,
		disabled = false,
		onsave,
	}: {
		naming: MapNaming;
		disabled?: boolean;
		onsave: (naming: MapNaming) => void;
	} = $props();

	const buffer = draftOf(() => naming);
	const dirty = $derived(buffer.dirty);

	const STRATEGIES = ['chain', 'siblings'] as const satisfies readonly NamingStrategyKind[];
	const ALIAS_SCHEMES = ['numeric', 'alphabetical'] as const satisfies readonly AliasScheme[];
	const scheme = $derived(buffer.value.alias_scheme);

	/** The draft, settled for a moment, so typing does not ask the server on every key. */
	const settled = debounced(() => ({ ...buffer.value }), 200);
	const previewQuery = createQuery(() => ({
		queryKey: ['naming-preview', settled.current],
		queryFn: () => api.previewNaming(settled.current),
		placeholderData: keepPreviousData,
	}));
	const preview = $derived(previewQuery.data ?? null);

	const FORMATS = [
		{
			key: 'bookmark_wormhole' as const,
			preview: 'wormhole' as const,
			label: 'Wormhole',
			help: 'A hole leading further into wormhole space.',
		},
		{
			key: 'bookmark_kspace' as const,
			preview: 'kspace' as const,
			label: 'K-space',
			help: 'A hole leading out to known space.',
		},
		{
			key: 'bookmark_return' as const,
			preview: 'return' as const,
			label: 'Return',
			help: 'The hole back the way you came.',
		},
	];
</script>

<Card.Root>
	<Card.Header>
		<Card.Title>Chain naming</Card.Title>
		<Card.Description>
			How aliases and bookmarks are generated for everyone on this map.
		</Card.Description>
	</Card.Header>
	<Card.Content>
		<Field.FieldGroup>
			<Field.Field>
				<Field.FieldLabel>Strategy</Field.FieldLabel>
				<ToggleGroup.Root
					type="single"
					variant="outline"
					value={buffer.value.strategy}
					{disabled}
					onValueChange={(value) => {
						const picked = oneOf(STRATEGIES, value);
						if (picked) buffer.value.strategy = picked;
					}}
					data-testid="naming-strategy"
				>
					<ToggleGroup.Item value="chain">Chain</ToggleGroup.Item>
					<ToggleGroup.Item value="siblings">Per system</ToggleGroup.Item>
				</ToggleGroup.Root>
				<Field.FieldDescription>
					{#if buffer.value.strategy === 'siblings'}
						Each system numbers its own holes, so one further out starts again. The map, not the
						alias, says which way home is, so set a home system.
					{:else}
						An alias extends the one it was reached from, so it carries the way home.
					{/if}
				</Field.FieldDescription>
			</Field.Field>

			<Field.Field>
				<Field.FieldLabel>Alias scheme</Field.FieldLabel>
				<ToggleGroup.Root
					type="single"
					variant="outline"
					value={buffer.value.alias_scheme}
					{disabled}
					onValueChange={(value) => {
						const picked = oneOf(ALIAS_SCHEMES, value);
						if (picked) buffer.value.alias_scheme = picked;
					}}
					data-testid="alias-scheme"
				>
					<ToggleGroup.Item value="numeric">Numeric</ToggleGroup.Item>
					<ToggleGroup.Item value="alphabetical">Alphabetical</ToggleGroup.Item>
				</ToggleGroup.Root>
				<Field.FieldDescription>
					Chains start <span class="font-mono" data-testid="alias-preview"
						>{preview?.aliases.join(', ') ?? ''}</span
					>
					{#if scheme === 'alphabetical'}
						. H, L, N and P are reserved for exits to known space.
					{/if}
				</Field.FieldDescription>
			</Field.Field>

			<Field.Field>
				<Field.FieldLabel for="ignored-alias">Home alias</Field.FieldLabel>
				<Input
					id="ignored-alias"
					bind:value={buffer.value.ignored_alias}
					placeholder={preview?.defaults.ignored_alias}
					{disabled}
					data-testid="ignored-alias"
				/>
				<Field.FieldDescription>
					Sits outside the chain: its holes start a fresh sequence, and anything pointing back at it
					is bookmarked as a way home.
				</Field.FieldDescription>
			</Field.Field>

			<Field.FieldSet>
				<Field.FieldLegend>Bookmark formats</Field.FieldLegend>
				<Field.FieldDescription>
					Available: {(preview?.tokens ?? []).map((t) => `{${t}}`).join(' ')}. Empty ones drop out.
				</Field.FieldDescription>
				<Field.FieldGroup>
					{#each FORMATS as format (format.key)}
						<Field.Field>
							<Field.FieldLabel for={format.key}>{format.label}</Field.FieldLabel>
							<Input
								id={format.key}
								bind:value={buffer.value[format.key]}
								placeholder={preview?.defaults[format.preview]}
								{disabled}
								class="font-mono"
								data-testid={format.key}
							/>
							<Field.FieldDescription>
								{format.help}
								<span class="font-mono text-foreground" data-testid="{format.key}-preview">
									{preview?.[format.preview] ?? ''}
								</span>
							</Field.FieldDescription>
						</Field.Field>
					{/each}
				</Field.FieldGroup>
			</Field.FieldSet>
		</Field.FieldGroup>
	</Card.Content>
	<Card.Footer class="justify-end gap-2">
		<Button variant="ghost" disabled={disabled || !dirty} onclick={() => buffer.reset()}>
			Reset
		</Button>
		<Button
			variant="outline"
			disabled={disabled || !dirty}
			onclick={() => onsave({ ...buffer.value })}
			data-testid="save-naming">Save</Button
		>
	</Card.Footer>
</Card.Root>
