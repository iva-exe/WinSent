<script>
	// Nabídka widgetů, které na ploše ještě nejsou.
	//
	// Ukazuje se jen v režimu úprav a je řazená po sekcích, protože
	// tak si uživatel widget hledá: „něco ze Sítě", ne „něco na S".
	import { Plus, Check } from 'lucide-svelte';
	import { REGISTR } from './registr.js';
	import { rozlozeni, pridej, obnovVychozi } from './rozlozeni.svelte.js';

	let hledani = $state('');

	let podleSekci = $derived.by(() => {
		const q = hledani.trim().toLowerCase();
		const skupiny = new Map();
		for (const w of Object.values(REGISTR)) {
			if (q && !`${w.nazev} ${w.sekce} ${w.popis ?? ''}`.toLowerCase().includes(q)) continue;
			if (!skupiny.has(w.sekce)) skupiny.set(w.sekce, []);
			skupiny.get(w.sekce).push(w);
		}
		return [...skupiny.entries()];
	});

	let naplose = $derived(new Set(rozlozeni.dlazdice.map((d) => d.id)));
</script>

<section class="pridat">
	<header>
		<span class="label-tech">// přidat dlaždici</span>
		<input
			bind:value={hledani}
			placeholder="hledat widget…"
			spellcheck="false"
			autocomplete="off"
		/>
		<button class="vychozi" onclick={obnovVychozi}>Výchozí sada</button>
	</header>

	{#each podleSekci as [sekce, widgety] (sekce)}
		<div class="skup">
			<h3>{sekce}</h3>
			<div class="polozky">
				{#each widgety as w (w.id)}
					{@const je = naplose.has(w.id)}
					<button class="polozka" class:je disabled={je} onclick={() => pridej(w.id)}>
						<w.ikona size={15} />
						<span class="pj">
							<span class="pn">{w.nazev}</span>
							{#if w.popis}<span class="pp">{w.popis}</span>{/if}
						</span>
						{#if je}<Check size={14} />{:else}<Plus size={14} />{/if}
					</button>
				{/each}
			</div>
		</div>
	{/each}

	{#if !podleSekci.length}
		<p class="nic">Nic takového tu není.</p>
	{/if}
</section>

<style>
	.pridat {
		border: 1px dashed var(--border-strong);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 12px 14px 14px;
		/* Nabídka je dlouhá přes čtyřicet položek. Bez vlastního
		   rolování by vytlačila plochu s dlaždicemi mimo okno a
		   uživatel by v režimu úprav neviděl to, co upravuje. */
		max-height: 40vh;
		overflow-y: auto;
		flex: none;
	}
	header {
		display: flex;
		align-items: center;
		gap: 10px;
		margin-bottom: 10px;
		/* Řádek s hledáním zůstává vidět i po odrolování nabídky. */
		position: sticky;
		top: -12px;
		padding-top: 2px;
		background: var(--bg);
		z-index: 1;
	}
	header input {
		flex: 1;
		min-width: 0;
		max-width: 260px;
		padding: 4px 9px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--panel);
		color: var(--text);
		font: inherit;
		font-size: var(--fs-sm);
		outline: none;
	}
	.vychozi {
		margin-left: auto;
		padding: 4px 10px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: none;
		color: var(--text-dim);
		font: inherit;
		font-size: var(--fs-xs);
		cursor: pointer;
	}
	.vychozi:hover {
		background: var(--surface-hover);
		color: var(--text);
	}
	.skup + .skup {
		margin-top: 10px;
	}
	h3 {
		margin: 0 0 5px;
		font-size: var(--fs-2xs);
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		color: var(--text-faint);
	}
	.polozky {
		display: grid;
		grid-template-columns: repeat(auto-fill, minmax(232px, 1fr));
		gap: 5px;
	}
	.polozka {
		display: flex;
		align-items: center;
		gap: 8px;
		padding: 6px 9px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		background: var(--panel);
		color: var(--text);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.polozka:hover:not(:disabled) {
		background: var(--surface-hover);
		border-color: var(--border-strong);
	}
	/* Co už na ploše je, zůstává vidět — jinak by uživatel nevěděl,
	   jestli widget neexistuje, nebo ho už má. */
	.polozka.je {
		opacity: 0.45;
		cursor: default;
	}
	.pj {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex: 1;
	}
	.pn {
		font-size: var(--fs-md);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.pp {
		font-size: var(--fs-2xs);
		color: var(--text-faint);
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.nic {
		margin: 0;
		font-size: var(--fs-sm);
		color: var(--text-dim);
	}
</style>
