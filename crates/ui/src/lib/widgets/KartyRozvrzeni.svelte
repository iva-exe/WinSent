<script>
	// Oddělovač — jediná dlaždice, která nic neměří.
	//
	// Plocha o dvaceti kartách je bez předělů jedna hromada. Tohle je
	// nadpis přes zvolený počet sloupců: napíšeš si do něj, co pod ním
	// je („Ráno se kouknu sem"), a přidat si ho můžeš kolikrát chceš —
	// proto je v registru označený jako vícenásobný.
	import { nastavText } from './rozlozeni.svelte.js';

	let { polozka, edit = false } = $props();
</script>

<div class="odd" class:prazdny={!polozka.text && !edit}>
	{#if edit}
		<input
			class="bez-tahu"
			value={polozka.text}
			placeholder="nadpis oddělovače…"
			spellcheck="false"
			autocomplete="off"
			maxlength="60"
			oninput={(e) => nastavText(polozka.klic, e.currentTarget.value)}
		/>
	{:else if polozka.text}
		<span class="text">{polozka.text}</span>
	{/if}
	<span class="cara"></span>
</div>

<style>
	.odd {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
	}
	.text {
		flex: none;
		font-size: var(--fs-sm);
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		max-width: 70%;
	}
	.cara {
		flex: 1;
		height: 0;
		border-top: 1px dotted var(--border-strong);
	}
	/* Oddělovač bez textu je pořád oddělovač — čára jde přes celou
	   šířku, ať je vidět, že tam něco je. */
	.prazdny .cara {
		flex: 1;
	}
	input {
		flex: none;
		width: min(260px, 60%);
		padding: 3px 8px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--panel);
		color: var(--text);
		font: inherit;
		font-size: var(--fs-sm);
		letter-spacing: 0.04em;
		outline: none;
	}
	input:focus {
		border-color: var(--border-strong);
	}
</style>
