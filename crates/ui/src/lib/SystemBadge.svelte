<script>
	// Jednotný štítek „součást Windows" — stejný vzhled napříč
	// sekcemi (Tasks, Programs, Files, Po spuštění).
	import { ShieldCheck } from 'lucide-svelte';

	/// `compact` = jen ikonka s tooltipem (do řádků seznamů);
	/// jinak celý badge s textem (do detailů).
	let { compact = false, label = 'součást Windows' } = $props();
</script>

{#if compact}
	<span class="sysb compact" title="Povinná součást Windows / Microsoft — neodinstalovávat">
		<ShieldCheck size={14} />
	</span>
{:else}
	<span class="sysb full" title="Povinná součást Windows / Microsoft — neodinstalovávat">
		<ShieldCheck size={15} />
		{label}
	</span>
{/if}

<style>
	.sysb {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--net-down);
		vertical-align: -2px;
	}
	.sysb.full {
		font-size: 0.7rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		border: 1px solid color-mix(in srgb, var(--net-down) 40%, transparent);
		border-radius: 999px;
		padding: 2px 8px;
	}
</style>
