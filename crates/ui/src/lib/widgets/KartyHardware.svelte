<script>
	// Dlaždice ze sekcí Hardware a Drivers — součástky a jejich stav.
	//
	// Pravidlo celé skupiny: co stroj nehlásí, to se nepředstírá.
	// Desktop nemá baterii, spousta desek nehlásí teplotu procesoru
	// a SATA disky neumí SMART přes standardní rozhraní. Widget v tom
	// případě řekne, že údaj není — číslo se nedopočítává.
	import { invoke } from '@tauri-apps/api/core';
	import { data } from './data.svelte.js';
	import { doba } from './pomoc.js';
	import { barvaTeploty } from '$lib/barvy.js';

	// Rozměry přicházejí v jednotkách mřížky: šířka ve sloupcích,
	// výška v řádcích (řádek je nízký, viz registr.js). Obsah se podle
	// nich rozhoduje, co se ještě vejde.
	let { typ, w = 1, h = 2 } = $props();

	let siroka = $derived(w >= 2);
	let hw = $derived(data.hardware);

	// ── teploty ──────────────────────────────────────────────────────
	let teploty = $derived.by(() => {
		const out = [];
		// Procesor živě ze snímku jako grafika (PawnIO, každou sekundu);
		// hardwarový přehled jen jako záloha (ACPI), ten chodí zřídka.
		const cpuZiva = data.system?.cpu_temp_c;
		const cpu = hw?.cpu_thermal;
		if (cpuZiva != null) out.push({ co: 'Procesor', c: cpuZiva, zdroj: 'PawnIO', druh: 'cpu' });
		else if (cpu?.celsius != null) out.push({ co: 'Procesor', c: cpu.celsius, zdroj: cpu.temp_source, druh: 'cpu' });
		// Teplota grafiky je z NVML, nebo (AMD/Intel, NVML bez teploty)
		// z D3DKMT; snímek zdroj nenese, obojí ale čte ovladač karty.
		const gpu = data.system?.gpu?.temp_c;
		if (gpu != null) out.push({ co: 'Grafika', c: gpu, zdroj: 'ovladač GPU', druh: 'gpu' });
		for (const d of data.volumes?.health ?? []) {
			if (d.temp_c != null) out.push({ co: d.model, c: d.temp_c, zdroj: 'SMART', druh: 'disk' });
		}
		return out;
	});
	// Barva podle druhu součástky ($lib/barvy.js) — disk má 60 °C už
	// teplých, procesor ne. Dřív tu platilo 70/85 pro všechno a NVMe
	// na 65 °C svítil zeleně, zatímco Hardware ho měl oranžově.

	// ── obrazovky ────────────────────────────────────────────────────
	// Jméno monitoru je u většiny sestav jen „Generic PnP Monitor",
	// což o obrazovce neříká nic. Hlavní řádek je proto režim; model
	// se doplní jen tehdy, když ho systém opravdu zná.
	let obrazovky = $derived(data.displays ?? []);
	function znameJmeno(m) {
		return m && !/generic|pnp|default/i.test(m) ? m : '';
	}

	// ── ovladače ─────────────────────────────────────────────────────
	let stare = $derived.by(() => {
		const rows = data.drivers?.drivers ?? [];
		return [...rows]
			.filter((d) => d.third_party)
			.map((d) => ({ ...d, rok: Number(String(d.date).match(/\d{4}/)?.[0] ?? 0) }))
			.filter((d) => d.rok > 0)
			.sort((a, b) => a.rok - b.rok)
			.slice(0, 4);
	});

	let hledam = $state(false);
	async function hledejBios() {
		const b = hw?.board;
		if (!b) return;
		hledam = true;
		try {
			await invoke('search_web', {
				query: `${b.manufacturer} ${b.product} BIOS update`.trim()
			});
		} catch {
			/* prohlížeč se neotevřel — nic dalšího s tím neuděláme */
		}
		hledam = false;
	}
</script>

{#if typ === 'deska'}
	{#if hw?.board?.product}
		<span class="w-mid">{hw.board.manufacturer} {hw.board.product}</span>
		<span class="w-sub">
			BIOS {hw.board.bios_version || '—'}
			{#if hw.board.bios_date}· {hw.board.bios_date}{/if}
		</span>
		{#if siroka && hw.board.system_product}
			<span class="w-sub">{hw.board.system_manufacturer} {hw.board.system_product}</span>
		{/if}
		<button class="w-akce" disabled={hledam} onclick={hledejBios}>Hledat aktualizaci BIOSu</button>
	{:else}
		<span class="w-empty">Deska se ještě nenačetla.</span>
	{/if}
{:else if typ === 'teploty'}
	{#if teploty.length}
		<ul class="w-list">
			{#each teploty as t, i (i)}
				<li class="w-row">
					<span class="w-name">{t.co}</span>
					<span class="w-sub">{t.zdroj}</span>
					<span class="w-mono" style:color={barvaTeploty(t.c, t.druh)}>{Math.round(t.c)} °C</span>
				</li>
			{/each}
		</ul>
	{:else}
		<span class="w-empty">Tenhle stroj teplotu nehlásí ani z jedné součástky.</span>
	{/if}
{:else if typ === 'moduly'}
	{@const mods = data.sysInfo?.ram_modules ?? []}
	{#if mods.length}
		<ul class="w-list scroll">
			{#each mods as m, i (i)}
				<li class="w-row">
					<span class="w-name">{m.slot}</span>
					<span class="w-sub">{m.manufacturer}</span>
					<span class="w-mono">{m.size_mb ? `${(m.size_mb / 1024).toFixed(0)} GB` : '?'}</span>
					<span class="w-mono w-dim" title={m.clock_mhz ? `takt ${m.clock_mhz} MHz` : ''}>
						{m.mem_type ? `${m.mem_type} ` : ''}{m.configured_mts || m.speed_mts} MT/s
					</span>
				</li>
			{/each}
		</ul>
		<span class="w-sub">
			{mods.length} z {data.sysInfo?.ram_slots ?? mods.length} slotů osazeno
		</span>
	{:else}
		<span class="w-empty">Moduly se ještě nenačetly.</span>
	{/if}
{:else if typ === 'obrazovky'}
	{#if obrazovky.length}
		<ul class="w-list">
			{#each obrazovky as d, i (i)}
				<li class="w-row">
					<span class="w-name">
						{znameJmeno(d.monitor) || d.adapter}
						{#if d.primary}<span class="w-dim">· hlavní</span>{/if}
					</span>
					<span class="w-mono">{d.width} × {d.height}</span>
					{#if d.refresh_hz}<span class="w-mono w-dim">{d.refresh_hz} Hz</span>{/if}
				</li>
			{/each}
		</ul>
	{:else}
		<span class="w-empty">Obrazovky se ještě nenačetly.</span>
	{/if}
{:else if typ === 'pagefile'}
	{#if hw?.pagefile}
		{@const p = hw.pagefile}
		<span class="w-big">{(p.used_mb / 1024).toFixed(1)}<span class="w-sub"> GB</span></span>
		<span class="w-sub">z {(p.size_mb / 1024).toFixed(0)} GB · vrchol {(p.peak_mb / 1024).toFixed(1)} GB</span>
		{#if siroka}<span class="w-sub">{p.path}</span>{/if}
	{:else}
		<span class="w-empty">Stránkovací soubor se nepodařilo přečíst.</span>
	{/if}
{:else if typ === 'baterie'}
	{#if hw?.battery}
		{@const b = hw.battery}
		<span class="w-big">{b.percent != null ? `${b.percent} %` : '—'}</span>
		<span class="w-sub">
			{b.charging ? 'nabíjí se' : b.ac_online ? 'v síti' : 'na baterii'}
			{#if b.remaining_s}· zbývá {doba(b.remaining_s)}{/if}
		</span>
		{#if b.wear_pct != null}
			<span class="w-sub">opotřebení {b.wear_pct.toFixed(0)} %{#if b.cycles}· {b.cycles} cyklů{/if}</span>
		{/if}
	{:else}
		<span class="w-empty">Tenhle počítač baterii nehlásí.</span>
	{/if}
{:else if typ === 'ovladace'}
	{@const d = data.drivers}
	{#if d}
		<div class="cisla">
			<span><b class="w-mono">{d.drivers.length}</b><span class="w-sub"> celkem</span></span>
			<span><b class="w-mono">{d.third_party}</b><span class="w-sub"> zvenčí</span></span>
			<span>
				<b class="w-mono" style:color={d.with_problem ? 'var(--warn)' : 'var(--text)'}>
					{d.with_problem}
				</b><span class="w-sub"> s problémem</span>
			</span>
		</div>
		{#if siroka && stare.length}
			<span class="w-sub">nejstarší doinstalované:</span>
			<ul class="w-list">
				{#each stare as s, i (i)}
					<li class="w-row">
						<span class="w-name">{s.device}</span>
						<span class="w-mono w-dim">{s.rok}</span>
					</li>
				{/each}
			</ul>
		{/if}
	{:else}
		<span class="w-empty">Ovladače se ještě nenačetly.</span>
	{/if}
{/if}

<style>
	.cisla {
		display: flex;
		flex-wrap: wrap;
		gap: 4px 14px;
		align-items: baseline;
		margin-bottom: 4px;
	}
	.cisla b {
		font-size: 1.15rem;
		font-weight: 500;
	}
</style>
