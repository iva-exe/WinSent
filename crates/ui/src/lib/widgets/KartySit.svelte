<script>
	// Dlaždice ze sekcí Network a Connection.
	//
	// Zásada ze SPEC 12.2 platí i tady: ukazuje se KAM a KOLIK, nikdy
	// CO. A adresy se nikam neposílají — ani do vyhledávače: je v nich
	// vidět, kam se uživatel připojuje.
	import { invoke } from '@tauri-apps/api/core';
	import { goto } from '$app/navigation';
	import AppIcon from '$lib/AppIcon.svelte';
	import MiniGraf from './MiniGraf.svelte';
	import { data, serie } from './data.svelte.js';
	import { ikony, chciIkonu } from './ikony.svelte.js';
	import { openMenu, akceKopirovat } from '$lib/itemmenu.svelte.js';
	import { bps, pocet } from './pomoc.js';

	let { typ, velikost: rozmer } = $props();

	let siroka = $derived(rozmer !== 'mala');
	let velka = $derived(rozmer === 'velka' || rozmer === 'siroka');

	let s = $derived(data.system);
	let site = $derived(data.network ?? []);
	let conn = $derived(data.connection);

	// ── kdo stahuje ──────────────────────────────────────────────────
	let stahuji = $derived(
		[...site]
			.filter((r) => r.rx_bps + r.tx_bps > 0)
			.sort((a, b) => b.rx_bps - a.rx_bps)
			.slice(0, velka ? 6 : 3)
	);
	$effect(() => {
		for (const r of stahuji) chciIkonu(r.identity_key);
	});

	let souhrnSpojeni = $derived.by(() => ({
		established: site.reduce((a, r) => a + r.established, 0),
		listening: site.reduce((a, r) => a + r.listening, 0),
		aplikaci: site.length
	}));

	// ── naslouchající porty ──────────────────────────────────────────
	// Loopback je port, na který se zvenčí nikdo nedostane — proto se
	// dá schovat. Co je z něj DOOPRAVDY dosažitelné přes internet, ví
	// jenom firewall, a ten v datech není; dlaždice proto slibuje jen
	// „mimo tento počítač".
	let jenVenku = $state(true);
	function jeLoopback(a) {
		return a === '::1' || a.startsWith('127.');
	}
	let porty = $derived.by(() => {
		const out = [];
		for (const r of site) {
			const listen = (r.conns ?? []).filter((c) => c.proto === 'tcp' && c.state === 'listen');
			const vybrane = jenVenku ? listen.filter((c) => !jeLoopback(c.local)) : listen;
			// Dynamické porty RPC jsou vždycky desítky čísel bez
			// významu; jako řádek na řádek by zaplnily celou dlaždici.
			const dyn = vybrane.filter((c) => c.local_port >= 49152);
			for (const c of vybrane.filter((c) => c.local_port < 49152)) {
				out.push({ key: `${r.identity_key}:${c.local}:${c.local_port}`, app: r.app_name, port: String(c.local_port), kde: c.local });
			}
			if (dyn.length) {
				out.push({
					key: `${r.identity_key}:dyn`,
					app: r.app_name,
					port: `${dyn.length} dynamických`,
					kde: 'RPC'
				});
			}
		}
		return out.sort((a, b) => a.app.localeCompare(b.app, 'cs')).slice(0, velka ? 8 : 4);
	});

	// ── připojení ────────────────────────────────────────────────────
	let adaptery = $derived((conn?.adapters ?? []).filter((a) => a.up && a.kind !== 'virtual'));
	let hlavni = $derived(adaptery.find((a) => a.ips?.length) ?? adaptery[0] ?? null);
	let wifi = $derived(conn?.wifi_connection ?? null);
	let vDosahu = $derived(
		[...(conn?.wifi_networks ?? [])].sort((a, b) => b.signal_pct - a.signal_pct).slice(0, velka ? 6 : 3)
	);

	function barvaSignalu(p) {
		if (p >= 70) return 'var(--ok)';
		if (p >= 40) return 'var(--warn)';
		return 'var(--danger)';
	}

	function menuAdresy(e, a) {
		openMenu(e, {
			title: a.name,
			subtitle: a.ips?.[0] ?? '',
			items: [
				...(a.ips?.[0] ? [akceKopirovat(a.ips[0], 'Kopírovat IP adresu')] : []),
				...(a.mac ? [akceKopirovat(a.mac, 'Kopírovat MAC adresu')] : [])
			]
		});
	}

	function nastaveniSite() {
		invoke('open_settings_page', { page: 'network' }).catch(() => {});
	}
</script>

{#if typ === 'prenos'}
	<span class="w-mid w-mono" style:color="var(--net-down)">↓ {bps(s?.net_rx_bps)}</span>
	<span class="w-mid w-mono" style:color="var(--net-up)">↑ {bps(s?.net_tx_bps)}</span>
	<MiniGraf
		values={serie.rx}
		values2={serie.tx}
		skala="auto"
		barva="var(--net-down)"
		vyska={siroka ? 40 : 26}
	/>
{:else if typ === 'stahuje'}
	<ul class="w-list scroll">
		{#each stahuji as r (r.identity_key)}
			<li>
				<button class="w-klik w-row" onclick={() => goto('/network?q=' + encodeURIComponent(r.app_name))}>
					<AppIcon src={ikony[r.identity_key]} name={r.app_name} size={15} />
					<span class="w-name">{r.app_name}</span>
					<span class="w-mono" style:color="var(--net-down)">{bps(r.rx_bps)}</span>
					{#if velka}<span class="w-mono" style:color="var(--net-up)">{bps(r.tx_bps)}</span>{/if}
				</button>
			</li>
		{/each}
		{#if !stahuji.length}<li class="w-empty">Zrovna nic nepřenáší data.</li>{/if}
	</ul>
{:else if typ === 'spojeni'}
	<span class="w-big">{souhrnSpojeni.established}</span>
	<span class="w-sub">aktivních spojení</span>
	<span class="w-sub">
		{pocet(souhrnSpojeni.aplikaci, 'aplikace', 'aplikace', 'aplikací')} · {souhrnSpojeni.listening} naslouchá
	</span>
{:else if typ === 'porty'}
	<div class="w-segs">
		<button class="w-seg" class:on={jenVenku} onclick={() => (jenVenku = true)}>Mimo tento počítač</button>
		<button class="w-seg" class:on={!jenVenku} onclick={() => (jenVenku = false)}>Všechny</button>
	</div>
	<ul class="w-list scroll">
		{#each porty as p (p.key)}
			<li class="w-row">
				<span class="w-name">{p.app}</span>
				<span class="w-mono w-dim">{p.port}</span>
			</li>
		{/each}
		{#if !porty.length}
			<li class="w-empty">
				{jenVenku ? 'Nic neposlouchá mimo tento počítač.' : 'Nic neposlouchá.'}
			</li>
		{/if}
	</ul>
{:else if typ === 'linka'}
	{#if hlavni}
		<span class="w-mid">{hlavni.description || hlavni.name}</span>
		<span class="w-sub">
			{hlavni.kind === 'wifi' ? 'WiFi' : hlavni.kind === 'ethernet' ? 'kabel' : hlavni.kind}
			{#if hlavni.link_mbps}· {hlavni.link_mbps >= 1000 ? `${(hlavni.link_mbps / 1000).toFixed(0)} Gb/s` : `${hlavni.link_mbps} Mb/s`}{/if}
			{#if hlavni.dhcp}· DHCP{/if}
		</span>
		{#if velka && adaptery.length > 1}
			<span class="w-sub">a další {adaptery.length - 1} připojené karty</span>
		{/if}
		<button class="w-akce" onclick={nastaveniSite}>Nastavení sítě ve Windows</button>
	{:else}
		<span class="w-empty">Žádná síťová karta není připojená.</span>
	{/if}
{:else if typ === 'adresa'}
	{#if hlavni}
		<!-- svelte-ignore a11y_no_static_element_interactions -->
		<div oncontextmenu={(e) => menuAdresy(e, hlavni)}>
			<span class="w-big adr w-mono">{hlavni.ips?.[0] ?? '—'}</span>
			<span class="w-sub">
				{#if hlavni.gateways?.length}brána {hlavni.gateways[0]}{/if}
				{#if siroka && hlavni.dns?.length}· DNS {hlavni.dns[0]}{/if}
			</span>
		</div>
	{:else}
		<span class="w-empty">Adresa zatím není přidělená.</span>
	{/if}
{:else if typ === 'signal'}
	{#if wifi}
		<span class="w-big" style:color={barvaSignalu(wifi.signal_pct)}>{wifi.signal_pct} %</span>
		<span class="w-sub">{wifi.ssid}{#if wifi.secured}· zabezpečená{/if}</span>
	{:else if conn?.wifi_present}
		<span class="w-empty">WiFi není připojená.</span>
	{:else}
		<span class="w-empty">Tenhle počítač WiFi kartu nemá.</span>
	{/if}
{:else if typ === 'site'}
	{#if vDosahu.length}
		<ul class="w-list scroll">
			{#each vDosahu as n, i (i)}
				<li class="w-row">
					<span class="w-name" class:pripojena={n.connected}>{n.ssid}</span>
					{#if !n.secured}<span class="w-sub" style:color="var(--warn)">otevřená</span>{/if}
					<span class="w-bar sig">
						<span
							class="w-fill"
							style:width="{n.signal_pct}%"
							style:background={barvaSignalu(n.signal_pct)}
						></span>
					</span>
				</li>
			{/each}
		</ul>
	{:else if conn?.wifi_present}
		<span class="w-empty">Služba zatím žádnou síť neviděla.</span>
	{:else}
		<span class="w-empty">Tenhle počítač WiFi kartu nemá.</span>
	{/if}
{/if}

<style>
	.adr {
		font-size: 1.2rem;
		display: block;
	}
	.sig {
		max-width: 54px;
		flex: none;
	}
	.pripojena {
		color: var(--ok);
	}
</style>
