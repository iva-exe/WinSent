<script>
	// Settings — v1 zatím jen dlaždice „Spotřeba nástroje“ (SPEC kap. 2.3):
	// rozpočet démona musí být pro uživatele ověřitelný, ne slibovaný.
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { daemon } from '$lib/daemon.svelte.js';
	import Num from '$lib/Num.svelte';
	import { prefs, setPref } from '$lib/prefs.svelte.js';

	let usage = $state(null);
	let error = $state('');

	async function refresh() {
		try {
			usage = await invoke('query_self_usage');
			error = '';
		} catch (e) {
			usage = null;
			error = String(e);
		}
	}

	function fmtBytes(b) {
		const mb = b / (1024 * 1024);
		return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(1)} MB`;
	}

	onMount(() => {
		refresh();
		const t = setInterval(refresh, 2000);
		return () => clearInterval(t);
	});
</script>

<div class="settings">
	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / spotřeba nástroje</span>
		</header>
		<p class="note">
			Démon se měří stejným samplerem jako každý jiný proces — žádná výjimka,
			žádné skrývání. Rozpočet: &lt; 0,5&nbsp;% CPU, &lt; 50&nbsp;MB RAM.
		</p>
		{#if usage}
			<div class="tiles">
				<div class="tile">
					<span class="label-tech">cpu</span>
					<span class="val value-mono" class:over={usage.cpu_pct > 0.5}>
						<Num value={usage.cpu_pct} decimals={2} suffix=" %" />
					</span>
				</div>
				<div class="tile">
					<span class="label-tech">ram</span>
					<span class="val value-mono" class:over={usage.ws_bytes > 50 * 1024 * 1024}>
						<Num value={usage.ws_bytes} format={fmtBytes} />
					</span>
				</div>
				<div class="tile">
					<span class="label-tech">databáze</span>
					<span class="val value-mono"><Num value={usage.db_bytes} format={fmtBytes} /></span>
				</div>
			</div>
		{:else}
			<p class="empty label-tech">{daemon.alive ? error || 'čekám na vzorek…' : 'služba neběží'}</p>
		{/if}
	</section>



	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / zobrazení</span>
		</header>
		<p class="note">
			Co se má v aplikaci ukazovat. Na systém to nesahá — Winsent nic nepřepíná
			ani neskrývá před Windows, je to jen volba zobrazení.
		</p>
		<!-- Přepínač je popiskem i tlačítkem zároveň: klik kamkoli na řádek
		     přepne, aby se nemuselo mířit na malý obdélníček. -->
		<button
			class="opt"
			role="switch"
			aria-checked={prefs.showZeroByte}
			onclick={() => setPref('showZeroByte', !prefs.showZeroByte)}
		>
			<span class="sw" class:on={prefs.showZeroByte}><span class="knob"></span></span>
			<span class="opt-text">
				<span class="opt-name">Ukazovat prázdné soubory ve Files</span>
				<span class="opt-why">
					Soubory o velikosti 0 B nejsou samy o sobě smetí — bývají to dočasné
					soubory aplikací, zámky nebo rozdělaná stahování. Ve výchozím stavu
					se proto neukazují.
				</span>
			</span>
		</button>
		<button
			class="opt"
			role="switch"
			aria-checked={prefs.showSystemStartup}
			onclick={() => setPref('showSystemStartup', !prefs.showSystemStartup)}
		>
			<span class="sw" class:on={prefs.showSystemStartup}><span class="knob"></span></span>
			<span class="opt-text">
				<span class="opt-name">Ukazovat startovací položky Windows</span>
				<span class="opt-why">
					V Po spuštění jsou vidět jen programy třetích stran — to, co s Windows
					startuje ze systému samotného, se nezobrazuje. Přepnout to stejně nejde
					(služba to odmítne) a dlouhý seznam nepřepínatelných řádků jen zakryje
					to, co ovlivnit můžeš. Zapnuté je uvidíš k náhledu, bez přepínače.
				</span>
			</span>
		</button>
	</section>
	<section class="card">
		<header class="card-head">
			<span class="label-tech">// settings / konfigurace</span>
		</header>
		<p class="note">
			Konfigurace žije v <span class="value-mono">%ProgramData%\syswatch\config.toml</span>
			a změny se projeví za běhu (hot-reload). Grafické nastavení přijde s dalšími verzemi.
		</p>
	</section>
</div>

<style>
	.settings {
		display: flex;
		flex-direction: column;
		gap: 1rem;
	}
	.card {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 0.9rem 1rem;
	}
	.card-head {
		margin-bottom: 0.6rem;
	}
	.note {
		margin: 0 0 0.8rem;
		color: var(--text-dim);
		font-size: var(--fs-lg);
	}
	/* Řádek s přepínačem. Celý řádek je tlačítko — mířit na malý
	   obdélníček je zbytečná práce navíc. */
	.opt {
		display: flex;
		align-items: flex-start;
		gap: 0.8rem;
		width: 100%;
		padding: 0.7rem 0.8rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--text);
		font: inherit;
		text-align: left;
		cursor: pointer;
	}
	.opt:hover {
		background: var(--surface-hover);
	}
	/* Přepínač — geometrie 1:1 s tím v Po spuštění, ať je to napříč
	   aplikací tentýž prvek, ne dva podobné. */
	.sw {
		position: relative;
		flex: none;
		width: 38px;
		height: 21px;
		margin-top: 1px;
		border-radius: 999px;
		border: 1px solid var(--border-strong);
		background: var(--panel);
		transition:
			background 0.18s ease,
			border-color 0.18s ease;
	}
	.sw .knob {
		position: absolute;
		top: 2px;
		left: 2px;
		width: 15px;
		height: 15px;
		border-radius: 50%;
		background: var(--text-faint);
		transition:
			transform 0.18s ease,
			background 0.18s ease;
	}
	.sw.on {
		background: color-mix(in srgb, var(--ok) 26%, transparent);
		border-color: color-mix(in srgb, var(--ok) 55%, transparent);
	}
	.sw.on .knob {
		transform: translateX(17px);
		background: var(--ok);
		box-shadow: var(--glow-ok);
	}
	.opt-text {
		display: flex;
		flex-direction: column;
		gap: 0.25rem;
	}
	.opt-name {
		font-size: var(--fs-xl);
	}
	.opt-why {
		font-size: var(--fs-md);
		color: var(--text-dim);
		line-height: 1.5;
	}
	.tiles {
		display: flex;
		gap: 0.8rem;
	}
	.tile {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		min-width: 130px;
		padding: 0.7rem 0.9rem;
		border: 1px dotted var(--border-strong);
		border-radius: var(--radius);
	}
	.tile .val {
		font-size: 1.15rem;
		color: var(--accent);
	}
	/* Překročený rozpočet — jediný význam, kdy tu smí být barva. */
	.tile .val.over {
		color: var(--warn);
	}
	.empty {
		margin: 0.6rem 0;
	}
</style>
