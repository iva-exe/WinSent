<script>
	// Connection (v9, SPEC kap. 12) — adaptéry, IP konfigurace, WiFi.
	//
	// Jen čtení: seznam karet jako v Hardware (ikona, název větší,
	// fakta jako štítky, stav vpravo). WiFi sekce se ukazuje jen na
	// stroji, který WiFi má — nic se nepředstírá. Připojování
	// a zapomínání sítí je správa; přijde přes validační vrstvu
	// za potvrzením, ne v téhle čtecí verzi.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { openMenu, akceKopirovat, akceOtevritUmisteni, oddelovac } from '$lib/itemmenu.svelte.js';
	import { Cable, Globe, Lock, LockOpen, Network, Wifi, WifiOff } from 'lucide-svelte';

	let report = $state(null);
	let loadError = $state('');

	async function load() {
		try {
			report = await invoke('query_connection');
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	onMount(() => {
		load();
		// Konfigurace se mění zřídka; signál WiFi za to stojí.
		const t = setInterval(load, 5000);
		return () => clearInterval(t);
	});

	// Fyzické adaptéry nahoru už řadí služba; virtuální bez linky
	// jsou šum — schované za počtem.
	let physical = $derived(
		(report?.adapters ?? []).filter((a) => a.kind === 'ethernet' || a.kind === 'wifi')
	);
	let other = $derived(
		(report?.adapters ?? []).filter((a) => a.kind !== 'ethernet' && a.kind !== 'wifi')
	);
	let showOther = $state(false);

	// Kontextové menu adaptéru.
	//
	// Jméno adaptéru je uživatelské („Ethernet"), zajímavý je popis
	// z ovladače („Realtek PCIe GbE Family Controller") — ten `openMenu`
	// vybere sám, protože „Ethernet" je v seznamu obecných slov.
	//
	// IP adresy se do vyhledávače neposílají: prozrazují topologii sítě
	// a v záznamu se maskují právě proto.
	function menuAdapter(e, a) {
		openMenu(e, {
			title: a.name,
			subtitle: a.description ?? '',
			hledat: [a.name, a.description],
			items: [
				{
					label: 'Nastavení sítě ve Windows',
					icon: 'shield',
					run: () => invoke('open_settings_page', { page: 'network' })
				},
				// Gigabitový řadič na stovce znamená kabel nebo protějšek.
				a.up && a.link_mbps && a.link_mbps <= 100
					? {
							label: 'Proč jede linka jen 100 Mb/s?',
							icon: 'search',
							hint: `${a.link_mbps} Mb/s`,
							run: () =>
								invoke('search_web', {
									query: 'gigabit ethernet běží jen 100 Mbps příčiny kabel'
								})
						}
					: null,
				oddelovac,
				akceKopirovat(a.description || a.name),
				akceKopirovat(a.mac, 'Kopírovat MAC adresu')
			]
		});
	}

	function kindLabel(k) {
		return k === 'ethernet' ? 'kabel' : k === 'wifi' ? 'WiFi' : k === 'virtual' ? 'virtuální' : 'jiný';
	}

	function speed(mbps) {
		if (!mbps) return null;
		return mbps >= 1000 ? (mbps / 1000).toFixed(mbps % 1000 ? 1 : 0) + ' Gb/s' : mbps + ' Mb/s';
	}

	// IPv4 první — to hledá člověk nejčastěji.
	function sortIps(ips) {
		return [...ips].sort((a, b) => a.includes(':') - b.includes(':'));
	}

	function signalClass(pct) {
		return pct >= 60 ? 'ok' : pct >= 35 ? 'warn' : 'bad';
	}
</script>

<div class="page">
	<header class="head">
		<h1>Connection</h1>
		<span class="label-tech">
			{physical.filter((a) => a.up).length} aktivních · {report?.adapters?.length ?? 0} adaptérů
		</span>
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst stav připojení: {loadError}</p>
	{:else if report}
		<div class="body">
			<!-- ── Fyzické adaptéry ── -->
			{#each physical as a (a.name + a.mac)}
				<article class="item" class:down={!a.up} oncontextmenu={(e) => menuAdapter(e, a)}>
					<div class="ico">
						{#if a.kind === 'wifi'}<Wifi size={19} />{:else}<Cable size={19} />{/if}
					</div>
					<div class="info">
						<h3>{a.name}</h3>
						<p class="vendor">{a.description}</p>
						<div class="facts">
							{#each sortIps(a.ips) as ip (ip)}
								<span class="fact mono">{ip}</span>
							{/each}
							{#if a.gateways.length}
								<span class="fact">brána {sortIps(a.gateways)[0]}</span>
							{/if}
							{#if a.dns.length}
								<span class="fact">DNS {sortIps(a.dns).slice(0, 2).join(', ')}</span>
							{/if}
							{#if a.up}
								<span class="fact muted">{a.dhcp ? 'adresa z DHCP' : 'statická adresa'}</span>
							{/if}
							{#if a.mac}<span class="fact mono muted">{a.mac}</span>{/if}
						</div>
					</div>
					<div class="side">
						{#if speed(a.link_mbps) && a.up}
							<span class="metric-sm">{speed(a.link_mbps)}</span>
						{/if}
						{#if a.up}
							<span class="pill quiet">připojeno</span>
						{:else}
							<span class="pill dim">odpojeno</span>
						{/if}
					</div>
				</article>
			{/each}

			<!-- ── WiFi: jen na stroji, který ji má ── -->
			{#if report.wifi_present}
				{#if report.wifi_connection}
					{@const c = report.wifi_connection}
					<h2 class="sect"><Wifi size={16} /> Připojená síť</h2>
					<article class="item">
						<div class="ico"><Wifi size={19} /></div>
						<div class="info">
							<h3>{c.ssid}</h3>
							<div class="facts">
								<span class="fact">
									{#if c.secured}zabezpečená síť{:else}nezabezpečená síť{/if}
								</span>
							</div>
						</div>
						<div class="side">
							<span class="metric-sm {signalClass(c.signal_pct)}">{c.signal_pct} %</span>
							<span class="pill quiet">signál</span>
						</div>
					</article>
				{/if}
				{#if report.wifi_networks.length}
					<h2 class="sect">
						<Globe size={16} /> Sítě v dosahu
						<span class="sect-n">{report.wifi_networks.length}</span>
					</h2>
					<table>
						<thead>
							<tr><th>Síť</th><th>Signál</th><th>Zabezpečení</th></tr>
						</thead>
						<tbody>
							{#each report.wifi_networks as n (n.ssid)}
								<tr>
									<td>
										{n.ssid}
										{#if n.connected}<span class="tag">připojeno</span>{/if}
									</td>
									<td>
										<span class="sig {signalClass(n.signal_pct)}">{n.signal_pct} %</span>
									</td>
									<td class="dim">
										{#if n.secured}
											<Lock size={13} /> zabezpečená
										{:else}
											<LockOpen size={13} /> otevřená
										{/if}
									</td>
								</tr>
							{/each}
						</tbody>
					</table>
				{/if}
			{:else}
				<p class="nowifi">
					<WifiOff size={15} /> Tenhle počítač nemá WiFi adaptér — sekce sítí se proto
					neukazuje.
				</p>
			{/if}

			<!-- ── Virtuální a ostatní adaptéry: sbalené, jsou to šum ── -->
			{#if other.length}
				<button class="other-toggle label-tech" onclick={() => (showOther = !showOther)}>
					// virtuální a ostatní adaptéry ({other.length}) {showOther ? '▴' : '▾'}
				</button>
				{#if showOther}
					{#each other as a (a.name + a.mac)}
						<article class="item" class:down={!a.up}>
							<div class="ico"><Network size={19} /></div>
							<div class="info">
								<h3>{a.name}</h3>
								<p class="vendor">{a.description}</p>
								<div class="facts">
									<span class="fact muted">{kindLabel(a.kind)}</span>
									{#each sortIps(a.ips) as ip (ip)}
										<span class="fact mono muted">{ip}</span>
									{/each}
								</div>
							</div>
							<div class="side">
								{#if a.up}
									<span class="pill quiet">aktivní</span>
								{:else}
									<span class="pill dim">neaktivní</span>
								{/if}
							</div>
						</article>
					{/each}
				{/if}
			{/if}
		</div>
	{/if}
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 10px;
		height: 100%;
		min-height: 0;
	}
	.head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.head h1 {
		font-size: 1.2rem;
		font-weight: 600;
		margin: 0;
	}
	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-right: 6px;
	}

	.sect {
		display: flex;
		align-items: center;
		gap: 9px;
		margin: 20px 0 9px;
		font-family: var(--font-mono);
		font-size: var(--fs-md);
		font-weight: 500;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		color: var(--text-dim);
	}
	.sect::after {
		content: '';
		flex: 1;
		height: 1px;
		background: var(--border);
	}
	.sect-n {
		font-weight: 400;
		font-size: var(--fs-xs);
		color: var(--text-faint);
		font-variant-numeric: tabular-nums;
	}

	/* Karta adaptéru — stejný tvar jako karty v Hardware. */
	.item {
		display: grid;
		grid-template-columns: 40px minmax(0, 1fr) minmax(120px, auto);
		gap: 14px;
		align-items: start;
		padding: 14px 16px;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		margin-bottom: 8px;
		background: var(--surface);
	}
	.item:hover {
		background: var(--surface-hover);
	}
	.item.down {
		opacity: 0.6;
	}
	.ico {
		display: grid;
		place-items: center;
		width: 40px;
		height: 40px;
		border-radius: 11px;
		background: var(--surface-hover);
		color: var(--text-dim);
	}
	.info {
		min-width: 0;
	}
	.info h3 {
		margin: 0;
		font-size: 1.06rem;
		font-weight: 600;
		line-height: 1.3;
		word-break: break-word;
	}
	.vendor {
		margin: 3px 0 0;
		font-size: var(--fs-md);
		color: var(--text-dim);
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: 7px 8px;
		margin-top: 9px;
	}
	.fact {
		font-size: var(--fs-sm);
		line-height: 1.4;
		padding: 4px 11px;
		border-radius: 7px;
		background: var(--surface-hover);
		color: var(--text);
	}
	.fact.muted {
		background: none;
		padding-left: 2px;
		padding-right: 2px;
		color: var(--text-dim);
	}
	.fact.mono,
	.mono {
		font-family: var(--font-mono);
		font-size: var(--fs-sm);
	}
	.side {
		display: flex;
		flex-direction: column;
		align-items: flex-end;
		gap: 7px;
		text-align: right;
	}
	.metric-sm {
		font-size: 1.15rem;
		font-weight: 600;
		line-height: 1;
		font-variant-numeric: tabular-nums;
	}
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		font-size: var(--fs-sm);
		padding: 4px 11px;
		border-radius: 999px;
		border: 1px solid transparent;
		white-space: nowrap;
	}
	.pill.quiet {
		color: var(--text-dim);
		background: var(--surface-hover);
		border-color: var(--border);
	}
	.pill.quiet::before {
		content: '';
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--ok);
	}
	.pill.dim {
		color: var(--text-dim);
		background: var(--surface-hover);
	}

	.ok {
		color: var(--ok);
	}
	.warn {
		color: var(--warn);
	}
	.bad {
		color: var(--danger);
	}
	.dim {
		color: var(--text-dim);
	}
	.tag {
		font-size: var(--fs-2xs);
		padding: 1px 7px;
		margin-left: 6px;
		border-radius: 999px;
		border: 1px solid var(--border-strong);
		color: var(--text-dim);
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: var(--fs-lg);
	}
	th {
		text-align: left;
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		text-transform: uppercase;
		letter-spacing: 0.06em;
		font-weight: 500;
		color: var(--text-faint);
		padding: 4px 10px 4px 0;
		border-bottom: 1px solid var(--border);
	}
	td {
		padding: 6px 10px 6px 0;
		border-bottom: 1px solid var(--border);
	}
	.sig {
		font-variant-numeric: tabular-nums;
	}

	.nowifi {
		display: flex;
		align-items: center;
		gap: 8px;
		margin: 14px 0 0;
		font-size: var(--fs-md);
		color: var(--text-dim);
	}
	.other-toggle {
		display: block;
		background: none;
		border: none;
		cursor: pointer;
		padding: 14px 2px 6px;
		text-align: left;
	}
	.other-toggle:hover {
		color: var(--text);
	}
	.empty {
		color: var(--text-dim);
		font-size: var(--fs-lg);
		padding: 14px 0;
	}
</style>
