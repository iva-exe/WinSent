<script>
	// Ikona aplikace se záložním monogramem. Část položek (KB
	// aktualizace, runtime knihovny, jazykové balíčky) ikonu nemá
	// nikde v systému — prázdný rámeček vypadá jako chyba, monogram
	// s barvou z názvu ne.
	let { src = null, name = '', size = 18 } = $props();

	let letter = $derived((name.trim()[0] ?? '?').toUpperCase());
	// Odstín z názvu — stabilní, ať se ikona při překreslení nemění.
	let hue = $derived.by(() => {
		let h = 0;
		for (const c of name) h = (h * 31 + c.charCodeAt(0)) % 360;
		return h;
	});
</script>

{#if src}
	<img class="ai" style:width="{size}px" style:height="{size}px" {src} alt="" />
{:else}
	<span
		class="ai mono-fallback"
		style:width="{size}px"
		style:height="{size}px"
		style:font-size="{Math.round(size * 0.5)}px"
		style:background="hsl({hue} 22% 26%)"
		style:color="hsl({hue} 45% 78%)"
		aria-hidden="true">{letter}</span
	>
{/if}

<style>
	.ai {
		flex: none;
		border-radius: 3px;
		display: inline-grid;
		place-items: center;
	}
	.mono-fallback {
		font-family: var(--font-ui);
		font-weight: 600;
		line-height: 1;
		user-select: none;
	}
</style>
