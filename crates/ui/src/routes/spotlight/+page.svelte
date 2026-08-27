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

	onMount(() => {
		const un = listen('spotlight:route', () => {
			// Okno se vyvolalo znovu — začít s prázdným polem, ne tam,
			// kde uživatel skončil minule.
			hledani?.vycisti();
			hledani?.zaostri();
		});
		hledani?.zaostri();
		window.addEventListener('keydown', klavesa);
		return () => {
			un.then((f) => f());
			window.removeEventListener('keydown', klavesa);
		};
	});
</script>

<div class="spot">
	<FileSearch bind:this={hledani} compact onhotovo={zavri} />
</div>

<style>
	/* Stejný podklad jako aplikace, jen bez jejího rámu. Okno samo je
	   průhledné a bez dekorací, takže zaoblení i krytí musí zajistit
	   tenhle prvek — jinak by kolem zůstaly ostré rohy. */
	.spot {
		display: flex;
		flex-direction: column;
		height: 100vh;
		overflow: hidden;
		border: 1px solid var(--border-strong);
		border-radius: 14px;
		background: #1c1d24;
		background: color-mix(in srgb, #1c1d24 92%, transparent);
		backdrop-filter: blur(48px) saturate(150%);
		-webkit-backdrop-filter: blur(48px) saturate(150%);
		box-shadow: 0 24px 64px rgba(0, 0, 0, 0.6);
	}
</style>
