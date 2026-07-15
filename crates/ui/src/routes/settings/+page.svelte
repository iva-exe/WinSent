<script>
	// Settings — v1 zatím jen dlaždice „Spotřeba nástroje“ (SPEC kap. 2.3):
	// rozpočet démona musí být pro uživatele ověřitelný, ne slibovaný.
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { daemon } from '$lib/daemon.svelte.js';

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
						{usage.cpu_pct.toFixed(2)} %
					</span>
				</div>
				<div class="tile">
					<span class="label-tech">ram</span>
					<span class="val value-mono" class:over={usage.ws_bytes > 50 * 1024 * 1024}>
						{fmtBytes(usage.ws_bytes)}
					</span>
				</div>
				<div class="tile">
					<span class="label-tech">databáze</span>
					<span class="val value-mono">{fmtBytes(usage.db_bytes)}</span>
				</div>
			</div>
		{:else}
			<p class="empty label-tech">{daemon.alive ? error || 'čekám na vzorek…' : 'služba neběží'}</p>
		{/if}
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
		font-size: 0.85rem;
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
