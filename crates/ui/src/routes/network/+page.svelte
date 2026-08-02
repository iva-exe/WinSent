<script>
	// Network (v9, SPEC kap. 12) — spojení per aplikace.
	//
	// „Chrome má 40 spojení, kam vedou" — Task Manager tohle per
	// aplikace neumí. Seznam aplikací vlevo, spojení vybrané aplikace
	// vpravo; stejná stavba jako Programs (jedna aplikace, jeden jazyk).
	//
	// Zásady ze SPEC 12.2: ukazujeme KAM a KOLIK, nikdy CO — obsah
	// paketů se nečte. A signály, ne verdikty: naslouchající port je
	// informace („otevřená brána dovnitř"), ne poplach.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { Search, ArrowUpRight, Ear, Network as NetIcon } from 'lucide-svelte';
	import AppIcon from '$lib/AppIcon.svelte';

	let rows = $state([]);
	let filter = $state('');
	let selectedKey = $state(null);
	let loadError = $state('');

	// Ikony aplikací — stejný mechanismus jako Tasks a Programs.
	let iconUrls = $state({});
	const iconState = new Map();

	function rgbaToUrl(icon) {
		const c = document.createElement('canvas');
		c.width = icon.w;
		c.height = icon.h;
		const ctx = c.getContext('2d');
		const img = new ImageData(new Uint8ClampedArray(icon.rgba), icon.w, icon.h);
		ctx.putImageData(img, 0, 0);
		return c.toDataURL();
	}

	async function fetchIcon(key) {
		if (key.startsWith('pid:')) return;
		const st = iconState.get(key) ?? 0;
		if (st >= 6 || st === 'done') return;
		iconState.set(key, st + 1);
		try {
			const icon = await invoke('query_icon', { identityKey: key });
			if (icon) {
				iconUrls[key] = rgbaToUrl(icon);
				iconState.set(key, 'done');
			}
		} catch {
			/* služba mimo — zkusí se příště */
		}
	}

	async function load() {
		try {
			rows = await invoke('query_network');
			loadError = '';
			for (const r of rows.slice(0, 100)) fetchIcon(r.identity_key);
		} catch (e) {
			loadError = String(e);
		}
	}

	onMount(() => {
		load();
		// Snapshot tabulky spojení po 2 s je levný (SPEC 12.3); PTR
		// jména se doplňují postupně, jak je resolver na pozadí stíhá.
		const t = setInterval(load, 2000);
		return () => clearInterval(t);
	});

	let shown = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		if (!q) return rows;
		return rows.filter(
			(r) =>
				r.app_name.toLowerCase().includes(q) ||
				(r.publisher ?? '').toLowerCase().includes(q) ||
				r.conns.some(
					(c) => c.remote.includes(q) || (c.remote_name ?? '').toLowerCase().includes(q)
				)
		);
	});

	let selected = $derived(rows.find((r) => r.identity_key === selectedKey) ?? null);

	let totals = $derived.by(() => {
		let est = 0;
		let listen = 0;
		for (const r of rows) {
			est += r.established;
			listen += r.listening;
		}
		return { est, listen, apps: rows.length };
	});

	// Spojení vybrané aplikace seskupená podle cíle — 40 spojení na
	// tentýž server je jeden řádek s počtem, ne 40 řádků.
	let remoteGroups = $derived.by(() => {
		if (!selected) return [];
		const map = new Map();
		for (const c of selected.conns) {
			if (!c.remote || c.state === 'listen' || c.state === 'udp') continue;
			if (!map.has(c.remote)) {
				map.set(c.remote, {
					remote: c.remote,
					name: c.remote_name,
					ports: new Set(),
					count: 0
				});
			}
			const g = map.get(c.remote);
			g.count += 1;
			g.ports.add(c.remote_port);
			if (!g.name && c.remote_name) g.name = c.remote_name;
		}
		return [...map.values()].sort((a, b) => b.count - a.count);
	});

	let listeningPorts = $derived.by(() => {
		if (!selected) return [];
		return selected.conns
			.filter((c) => c.state === 'listen' || c.state === 'udp')
			.toSorted((a, b) => a.local_port - b.local_port);
	});

	function portsLabel(set) {
		const ports = [...set].sort((a, b) => a - b);
		return ports.length > 4 ? ports.slice(0, 4).join(', ') + '…' : ports.join(', ');
	}
</script>

<div class="page">
	<header class="head">
		<h1>Network</h1>
		<span class="label-tech">
			{totals.apps} aplikací · {totals.est} aktivních · {totals.listen} naslouchá
		</span>
		<div class="filter">
			<Search size={16} />
			<input placeholder="hledat aplikaci, adresu, doménu…" bind:value={filter} />
		</div>
	</header>

	<p class="note">
		Ukazujeme <strong>kam a kolik</strong> — obsah přenosů se nečte nikdy. Jména serverů jsou
		z reverzního DNS a doplňují se postupně.
	</p>

	{#if loadError}
		<p class="empty">Nelze načíst spojení: {loadError}</p>
	{:else}
		<div class="cols">
			<ul class="list">
				{#each shown as r (r.identity_key)}
					<li>
						<button
							class="row"
							class:active={selectedKey === r.identity_key}
							onclick={() => (selectedKey = r.identity_key)}
						>
							<AppIcon src={iconUrls[r.identity_key]} name={r.app_name} size={20} />
							<span class="row-main">
								<span class="row-title">{r.app_name}</span>
								<span class="row-pub">{r.publisher ?? '—'}</span>
							</span>
							<span class="row-stats">
								{#if r.established}
									<span class="stat" title="aktivní spojení">
										<ArrowUpRight size={14} />{r.established}
									</span>
								{/if}
								{#if r.listening}
									<span class="stat dim" title="naslouchající porty">
										<Ear size={14} />{r.listening}
									</span>
								{/if}
							</span>
						</button>
					</li>
				{:else}
					<li class="empty">{filter ? 'Nic neodpovídá hledání.' : 'Načítám spojení…'}</li>
				{/each}
			</ul>

			<section class="detail">
				{#if !selected}
					<div class="d-none-sel">
						<NetIcon size={30} />
						<p>Vyber aplikaci — uvidíš, kam se připojuje a na čem naslouchá.</p>
					</div>
				{:else}
					<div class="d-head">
						<AppIcon src={iconUrls[selected.identity_key]} name={selected.app_name} size={30} />
						<div>
							<h2>{selected.app_name}</h2>
							<p class="d-sub">
								{selected.publisher ?? '—'} · {selected.proc_count}
								{selected.proc_count === 1
									? 'proces'
									: selected.proc_count < 5
										? 'procesy'
										: 'procesů'}
								· {selected.conns.length}
								{selected.conns.length === 1
									? 'záznam'
									: selected.conns.length < 5
										? 'záznamy'
										: 'záznamů'}
							</p>
						</div>
					</div>

					{#if remoteGroups.length}
						<h3 class="label-tech">// kam se připojuje ({remoteGroups.length})</h3>
						<table>
							<thead>
								<tr><th>Server</th><th>Adresa</th><th>Porty</th><th>Spojení</th></tr>
							</thead>
							<tbody>
								{#each remoteGroups as g (g.remote)}
									<tr>
										<td class="host">
											{#if g.name}{g.name}{:else}<span class="dim">jméno zatím neznáme</span>{/if}
										</td>
										<td class="mono">{g.remote}</td>
										<td class="mono">{portsLabel(g.ports)}</td>
										<td class="num">{g.count}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{:else}
						<h3 class="label-tech">// kam se připojuje</h3>
						<p class="dim d-empty">Žádná aktivní odchozí spojení.</p>
					{/if}

					{#if listeningPorts.length}
						<h3 class="label-tech">// na čem naslouchá ({listeningPorts.length})</h3>
						<p class="d-note">
							Otevřená brána dovnitř — jiné programy nebo počítače se sem mohou připojit.
							U aplikací je to běžné (místní služby, synchronizace); je to informace, ne poplach.
						</p>
						<table>
							<thead>
								<tr><th>Protokol</th><th>Adresa</th><th>Port</th><th>PID</th></tr>
							</thead>
							<tbody>
								{#each listeningPorts as c, i (c.proto + c.local + c.local_port + i)}
									<tr>
										<td class="mono">{c.proto}</td>
										<td class="mono dim">{c.local}</td>
										<td class="mono">{c.local_port}</td>
										<td class="mono dim">{c.pid}</td>
									</tr>
								{/each}
							</tbody>
						</table>
					{/if}
				{/if}
			</section>
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
	.filter {
		margin-left: auto;
		display: flex;
		align-items: center;
		gap: 6px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 5px 9px;
		color: var(--text-dim);
		background: var(--surface);
		width: 300px;
	}
	.filter input {
		flex: 1;
		min-width: 0;
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: 0.85rem;
	}
	.note {
		margin: 0;
		font-size: 0.8rem;
		color: var(--text-dim);
	}
	.note strong {
		color: var(--text);
		font-weight: 500;
	}

	.cols {
		display: grid;
		grid-template-columns: minmax(360px, 460px) 1fr;
		gap: 14px;
		min-height: 0;
		flex: 1;
	}
	.list {
		list-style: none;
		margin: 0;
		padding: 0;
		overflow-y: auto;
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
	}
	.row {
		display: flex;
		align-items: center;
		gap: 10px;
		width: 100%;
		background: none;
		border: none;
		border-bottom: 1px solid var(--border);
		color: var(--text);
		font: inherit;
		text-align: left;
		padding: 9px 12px;
		cursor: pointer;
	}
	.row:hover {
		background: var(--surface-hover);
	}
	.row.active {
		background: var(--surface-hover);
		box-shadow: inset 2px 0 0 var(--accent);
	}
	.row-main {
		flex: 1;
		min-width: 0;
		display: flex;
		flex-direction: column;
		gap: 1px;
	}
	.row-title {
		font-size: 0.9rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row-pub {
		font-size: 0.74rem;
		color: var(--text-dim);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row-stats {
		display: flex;
		gap: 8px;
		flex: none;
	}
	.stat {
		display: inline-flex;
		align-items: center;
		gap: 3px;
		font-size: 0.8rem;
		font-variant-numeric: tabular-nums;
	}
	.stat.dim {
		color: var(--text-dim);
	}

	.detail {
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 16px;
		overflow-y: auto;
		min-height: 0;
	}
	.d-none-sel {
		height: 100%;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 10px;
		color: var(--text-faint);
		font-size: 0.85rem;
		text-align: center;
	}
	.d-head {
		display: flex;
		align-items: center;
		gap: 12px;
		margin-bottom: 14px;
	}
	.d-head h2 {
		margin: 0;
		font-size: 1.05rem;
		font-weight: 600;
	}
	.d-sub {
		margin: 2px 0 0;
		font-size: 0.78rem;
		color: var(--text-dim);
	}
	h3.label-tech {
		margin: 16px 0 6px;
		font-weight: 500;
	}
	.d-note {
		margin: 0 0 8px;
		font-size: 0.76rem;
		line-height: 1.4;
		color: var(--text-dim);
	}
	.d-empty {
		font-size: 0.82rem;
	}

	table {
		width: 100%;
		border-collapse: collapse;
		font-size: 0.82rem;
	}
	th {
		text-align: left;
		font-family: var(--font-mono);
		font-size: 0.66rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		font-weight: 500;
		color: var(--text-faint);
		padding: 4px 10px 4px 0;
		border-bottom: 1px solid var(--border);
	}
	td {
		padding: 5px 10px 5px 0;
		border-bottom: 1px solid var(--border);
		vertical-align: top;
	}
	.host {
		word-break: break-all;
	}
	.mono {
		font-family: var(--font-mono);
		font-size: 0.76rem;
	}
	.num {
		font-variant-numeric: tabular-nums;
	}
	.dim {
		color: var(--text-dim);
	}
	.empty {
		color: var(--text-dim);
		font-size: 0.84rem;
		padding: 14px;
	}
</style>
