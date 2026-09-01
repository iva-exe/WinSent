<script>
	// Home — nástěnka z dlaždic, kterou si uživatel skládá sám.
	//
	// Dřív to byla pevná mřížka sedmi karet. Teď je Home to jediné
	// místo v aplikaci, které si každý srovná po svém: co ho zajímá,
	// dá nahoru a zvětší, zbytek odebere. Sekce zůstávají tak, jak
	// jsou — dlaždice je zkratka do nich, ne jejich náhrada.
	//
	// Data si dlaždice nestahují samy. Přihlásí se o sadu (viz
	// data.svelte.js) a několik dlaždic nad touž sadou stojí jeden
	// dotaz — jinak by deset karet znamenalo deset dotazů za sekundu
	// na jedna a tatáž čísla.
	import { onMount } from 'svelte';
	import { Pencil, Check, LayoutGrid } from 'lucide-svelte';
	import Dlazdice from '$lib/widgets/Dlazdice.svelte';
	import Pridat from '$lib/widgets/Pridat.svelte';
	import { REGISTR } from '$lib/widgets/registr.js';
	import { rozlozeni, SLOUPCU, mrizka } from '$lib/widgets/rozlozeni.svelte.js';
	import { odebirej, dohon } from '$lib/widgets/data.svelte.js';
	import { sekceViditelna } from '$lib/prefs.svelte.js';

	let edit = $state(false);
	let plocha = $state(null);

	/// Dlaždice k vykreslení: platné id a sekce, kterou si uživatel
	/// nevypnul. Widget do vypnuté sekce by nabízel odkaz, který
	/// v navigaci není.
	let dlazdice = $derived(
		rozlozeni.dlazdice
			.map((d) => ({ ...d, w: REGISTR[d.id] }))
			.filter((d) => d.w && sekceViditelna(d.w.href))
	);

	// Jeden odběr za celý přehled. Kdyby se přihlašovala každá dlaždice
	// zvlášť, střídalo by se přihlášení a odhlášení při každé změně
	// rozložení a časovač by se pokaždé restartoval.
	let sady = $derived([...new Set(dlazdice.flatMap((d) => d.w.sady))].sort().join(','));
	$effect(() => {
		const klice = sady ? sady.split(',') : [];
		return odebirej(klice);
	});

	// Kolik sloupců se vejde. Pod 200 px na dlaždici už není co ukázat.
	function prepocti() {
		const w = plocha?.clientWidth ?? 0;
		if (!w) return;
		mrizka.sloupcu = Math.max(1, Math.min(SLOUPCU, Math.floor(w / 200)));
	}

	onMount(() => {
		prepocti();
		const ro = new ResizeObserver(prepocti);
		if (plocha) ro.observe(plocha);
		// Po probuzení okna (WebView2 se ve schovaném stavu uspí) se
		// všechny sady jednou dotáhnou, ať dlaždice neukazují stav
		// z doby, kdy se uživatel díval naposledy.
		const probuzeni = () => {
			if (!document.hidden) dohon();
		};
		document.addEventListener('visibilitychange', probuzeni);
		return () => {
			ro.disconnect();
			document.removeEventListener('visibilitychange', probuzeni);
		};
	});
</script>

<div class="page">
	<header class="head">
		<h1>Home</h1>
		<span class="sub">
			{edit ? 'přetažením přesuneš, čtvereček mění velikost' : 'souhrn systému — klik na dlaždici otevře sekci'}
		</span>
		<button class="rezim" class:on={edit} onclick={() => (edit = !edit)}>
			{#if edit}<Check size={14} /> Hotovo{:else}<Pencil size={14} /> Upravit{/if}
		</button>
	</header>

	{#if edit}
		<Pridat />
	{/if}

	<div class="plocha" bind:this={plocha}>
		<div class="grid" style:grid-template-columns="repeat({mrizka.sloupcu}, 1fr)">
			{#each dlazdice as d (d.id)}
				{@const Karta = d.w.komp}
				<Dlazdice widget={d.w} velikost={d.velikost} {edit}>
					<!-- Dlaždic je na ploše deset a čtou data z devíti různých
					     míst. Kdyby jedna spadla na neočekávaném tvaru
					     odpovědi, vzala by s sebou celý přehled — takhle
					     zůstane u své karty a zbytek jede dál. -->
					<svelte:boundary>
						<Karta typ={d.w.typ} velikost={d.velikost} />
						{#snippet failed(error)}
							<span class="w-empty" title={String(error)}>
								Tuhle dlaždici se nepodařilo vykreslit.
							</span>
						{/snippet}
					</svelte:boundary>
				</Dlazdice>
			{/each}
		</div>

		{#if !dlazdice.length}
			<div class="prazdno">
				<LayoutGrid size={22} />
				<p>Přehled je prázdný.</p>
				<button class="rezim on" onclick={() => (edit = true)}>Vybrat dlaždice</button>
			</div>
		{/if}
	</div>
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 12px;
		height: 100%;
		min-height: 0;
	}
	.head {
		display: flex;
		align-items: baseline;
		gap: 12px;
		flex: none;
	}
	.head h1 {
		font-size: 1.15rem;
		font-weight: 600;
	}
	.sub {
		color: var(--text-faint);
		font-size: var(--fs-sm);
	}
	.rezim {
		display: flex;
		align-items: center;
		gap: 5px;
		margin-left: auto;
		padding: 4px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: none;
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-sm);
		cursor: pointer;
	}
	.rezim:hover {
		background: var(--surface-hover);
		color: var(--text);
	}
	.rezim.on {
		border-color: var(--border-strong);
		background: var(--surface-hover);
		color: var(--text);
	}
	.plocha {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-right: 2px;
	}
	.grid {
		display: grid;
		/* Pevná výška řádku, ne auto: bez ní by dlaždice „přes dva
		   řádky" nebyla vyšší než ta vedle a přepínání velikostí by
		   nedělalo nic vidět. */
		grid-auto-rows: 108px;
		grid-auto-flow: row dense;
		gap: 10px;
		align-content: start;
	}
	.prazdno {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 8px;
		padding: 48px 0;
		color: var(--text-faint);
	}
	.prazdno p {
		margin: 0;
		font-size: var(--fs-md);
	}
</style>
