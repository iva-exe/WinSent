<script>
	// Spotlight lišta — jedna sekce aplikace jako samostatné okno.
	//
	// Okno zakládá a vystřeďuje hostitel (src-tauri/spotlight.rs); tady
	// je jen obsah. Kolem není nic z aplikace: žádná navigace, žádný
	// titulek, žádný křížek — vyvolá se zkratkou a zmizí, jakmile
	// přestane být vpředu nebo se zmáčkne Escape.
	//
	// Které sekci okno patří, říká událost `spotlight:route`. Zatím je
	// jediná (hledání), ale okno je na víc připravené — proto ten výběr.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { listen } from '@tauri-apps/api/event';
	import FileSearch from '$lib/FileSearch.svelte';

	let hledani = $state(null);

	async function zavri() {
		try {
			await invoke('hide_spotlight');
		} catch {
			/* okno se nezavřelo — nic víc s tím tady nesvedeme */
		}
	}

	function klavesa(e) {
		// Escape je jediná cesta ven zevnitř: okno nemá křížek.
		if (e.key === 'Escape') {
			e.preventDefault();
			zavri();
		}
	}

	// Okno se při zavření jen schovává, takže se komponenta neodmontuje
	// a `onMount` podruhé neproběhne — vyprázdnění potřebuje vnější
	// podnět. Podnětem je ZAMĚŘENÍ okna, ne vlastní událost: wry
	// přeposílá WM_SETFOCUS do webview, takže focus dorazí pokaždé, když
	// se lišta vyvolá, a nezávisí to na doručení zprávy ani na
	// oprávněních.
	//
	// Naletěl jsem na to: samotná událost `spotlight:route` se nikdy
	// nedoručila, protože okno lišty nespadalo do žádné Tauri capability
	// (`capabilities/default.json` má `windows: ["main"]`) a ACL
	// `plugin:event|listen` zamítlo. Slib skončil odmítnutý, v release
	// buildu bez konzole to nebylo nikde vidět a lišta se prostě
	// otvírala s tím, co v ní zbylo z minula. Capability teď existuje
	// (`capabilities/spotlight.json`), ale reset už na ní nestojí.
	let posledniProbuzeni = 0;
	function probud() {
		// Oba podněty chodí těsně po sobě; druhý by jen zbytečně znovu
		// tahal inventář a stav indexu přes pipe.
		const ted = Date.now();
		if (ted - posledniProbuzeni < 400) return;
		posledniProbuzeni = ted;
		hledani?.vycisti();
		hledani?.zaostri();
	}

	onMount(() => {
		window.addEventListener('focus', probud);
		// Záloha a zároveň cesta pro budoucí přepínání sekcí. `.catch`
		// je tu schválně — bez něj se zamítnutí od ACL ztratí
		// v neošetřeném slibu, což je přesně ta chyba, která tenhle
		// reset dlouho tiše vypínala.
		const un = listen('spotlight:route', probud).catch(() => () => {});
		hledani?.zaostri();
		window.addEventListener('keydown', klavesa);
		return () => {
			un.then((f) => f());
			window.removeEventListener('focus', probud);
			window.removeEventListener('keydown', klavesa);
		};
	});
</script>

<div class="spot">
	<FileSearch bind:this={hledani} compact onhotovo={zavri} />
</div>

<style>
	/* Okno samo je průhledné a bez dekorací, takže zaoblení i krytí
	   musí zajistit tenhle prvek — jinak by kolem zůstaly ostré rohy.

	   Rozostření sem NEPATŘÍ: dělá ho acrylic ve Windows, nastavený
	   při stavbě okna (src-tauri/spotlight.rs). `backdrop-filter` tu
	   dřív byl, ale nic nedělal — v průhledném okně nemá co filtrovat.

	   Krytí je proto výrazně nižší než dřívějších 92 %: přes skoro
	   neprůhledné pozadí by po efektu nebylo ani památky a okno by
	   působilo jako plný obdélník. Ladit se má primárně TADY (0,45
	   světlejší ↔ 0,70 tmavší), ne v Rustu — CSS působí na Windows 10
	   i 11 stejně, kdežto barva efektu se na jedenáctkách ignoruje. */
	.spot {
		display: flex;
		flex-direction: column;
		height: 100vh;
		overflow: hidden;
		border: 1px solid var(--border-strong);
		border-radius: 14px;
		background: rgba(28, 29, 36, 0.58);
		box-shadow: 0 24px 64px rgba(0, 0, 0, 0.6);
	}
</style>
