<script>
	// Dlaždice ze sekcí Programs a Vyhledávání — co je nainstalované
	// a co uživatel otevírá.
	//
	// Odinstalace ani úklid duchů se odsud nedělá: obojí je nevratné
	// a patří tam, kde je vidět plán a co přesně zmizí.
	import { invoke } from '@tauri-apps/api/core';
	import { goto } from '$app/navigation';
	import AppIcon from '$lib/AppIcon.svelte';
	import { data } from './data.svelte.js';
	import { ikony, chciIkonu } from './ikony.svelte.js';
	import { nacti as nactiPosledni, zapamatuj } from '$lib/recent.js';
	import { Folder, File } from 'lucide-svelte';
	import { den, pred } from './pomoc.js';

	let { typ, velikost: rozmer } = $props();

	let siroka = $derived(rozmer !== 'mala');
	let velka = $derived(rozmer === 'velka' || rozmer === 'siroka');
	let apps = $derived(data.apps ?? []);

	// ── inventář ─────────────────────────────────────────────────────
	let souhrn = $derived.by(() => ({
		vse: apps.length,
		desktop: apps.filter((a) => a.kind === 'desktop').length,
		msix: apps.filter((a) => a.kind === 'msix').length,
		os: apps.filter((a) => a.kind === 'os').length
	}));
	let bezi = $derived(data.system?.proc_count ?? null);

	let duchove = $derived(apps.filter((a) => a.missing_install));
	let bezOdinstalatoru = $derived(apps.filter((a) => a.uninstaller_missing).length);

	let nove = $derived(
		[...apps]
			.filter((a) => a.install_ts)
			.sort((a, b) => b.install_ts - a.install_ts)
			.slice(0, velka ? 6 : 3)
	);

	$effect(() => {
		for (const a of nove) chciIkonu(a.identity_key);
	});

	function doProgramu(a) {
		goto('/programs?q=' + encodeURIComponent(a.display_name));
	}

	// ── naposledy otevřené ───────────────────────────────────────────
	let posledni = $state([]);
	let spousti = $state('');
	let chyba = $state('');

	// Seznam píše hlavní okno i spotlight lišta do téhož úložiště,
	// takže se čte při každém zobrazení dlaždice znovu — jinak by tu
	// zůstalo pořadí z chvíle, kdy se Home otevřel.
	$effect(() => {
		posledni = nactiPosledni().slice(0, velka ? 8 : 4);
		const t = setInterval(() => {
			posledni = nactiPosledni().slice(0, velka ? 8 : 4);
		}, 5000);
		return () => clearInterval(t);
	});

	$effect(() => {
		for (const it of posledni) if (it.identity_key) chciIkonu(it.identity_key);
	});

	async function otevri(it) {
		if (spousti) return;
		spousti = it.key;
		chyba = '';
		try {
			if (it.kind === 'app') {
				await invoke('launch_app', {
					identityKey: it.identity_key,
					displayName: it.name,
					aumid: it.aumid || null
				});
			} else {
				await invoke('open_path', { path: it.path });
			}
			posledni = zapamatuj(it).slice(0, velka ? 8 : 4);
		} catch (e) {
			chyba = String(e);
		} finally {
			spousti = '';
		}
	}

	// ── stav inventáře ───────────────────────────────────────────────
	let stav = $state(null);
	async function nactiStav() {
		try {
			stav = await invoke('query_inv_status');
		} catch {
			stav = null;
		}
	}
	$effect(() => {
		if (typ !== 'sken') return;
		nactiStav();
		const t = setInterval(nactiStav, 10_000);
		return () => clearInterval(t);
	});
	async function preskenuj() {
		try {
			await invoke('rescan_apps');
			nactiStav();
		} catch {
			/* stav si přečteme při dalším tiku */
		}
	}
</script>

{#if typ === 'inventar'}
	<div class="cisla">
		<button class="c" onclick={() => goto('/programs')}>
			<b class="w-mono">{souhrn.vse}</b><span class="w-sub">nainstalováno</span>
		</button>
		<button class="c" onclick={() => goto('/programs')}>
			<b class="w-mono">{souhrn.desktop}</b><span class="w-sub">klasických</span>
		</button>
		<button class="c" onclick={() => goto('/programs')}>
			<b class="w-mono">{souhrn.msix}</b><span class="w-sub">z Microsoft Store</span>
		</button>
		{#if siroka}
			<button class="c" onclick={() => goto('/tasks')}>
				<b class="w-mono">{bezi ?? '—'}</b><span class="w-sub">procesů běží</span>
			</button>
		{/if}
	</div>
{:else if typ === 'duchove'}
	<span class="w-big" style:color={duchove.length ? 'var(--warn)' : 'var(--text)'}>
		{duchove.length}
	</span>
	<span class="w-sub">zápisů bez souborů na disku</span>
	{#if siroka}
		<ul class="w-list">
			{#each duchove.slice(0, 3) as a (a.identity_key)}
				<li>
					<button class="w-klik w-row" onclick={() => doProgramu(a)}>
						<span class="w-name">{a.display_name}</span>
					</button>
				</li>
			{/each}
		</ul>
		{#if bezOdinstalatoru}
			<span class="w-sub">{bezOdinstalatoru}× chybí i odinstalátor</span>
		{/if}
	{/if}
{:else if typ === 'nove'}
	<ul class="w-list scroll">
		{#each nove as a (a.identity_key)}
			<li>
				<button class="w-klik w-row" onclick={() => doProgramu(a)}>
					<AppIcon src={ikony[a.identity_key]} name={a.display_name} size={15} />
					<span class="w-name">{a.display_name}</span>
					<span class="w-mono w-dim">{den(a.install_ts)}</span>
				</button>
			</li>
		{/each}
		{#if !nove.length}<li class="w-empty">Žádná instalace nemá datum.</li>{/if}
	</ul>
{:else if typ === 'sken'}
	{#if stav}
		<span class="w-mid">{stav.scanning ? 'skenuji…' : 'inventář je hotový'}</span>
		<span class="w-sub">poslední sken {pred(stav.last_scan_ts)}</span>
	{:else}
		<span class="w-empty">Stav inventáře se nepodařilo přečíst.</span>
	{/if}
	<button class="w-akce" disabled={stav?.scanning} onclick={preskenuj}>Přeskenovat</button>
{:else if typ === 'naposledy'}
	<ul class="w-list scroll">
		{#each posledni as it (it.key)}
			<li>
				<button class="w-klik w-row" onclick={() => otevri(it)} disabled={!!spousti} title={it.path || it.name}>
					{#if it.kind === 'app'}
						<AppIcon src={ikony[it.identity_key]} name={it.name} size={15} />
					{:else if it.kind === 'dir'}
						<Folder size={15} color="var(--warn)" />
					{:else}
						<File size={15} color="var(--text-faint)" />
					{/if}
					<span class="w-name">{it.name}</span>
					<span class="w-sub">{it.sub}</span>
				</button>
			</li>
		{/each}
		{#if !posledni.length}
			<li class="w-empty">Zatím jsi nic z hledání neotevřel.</li>
		{/if}
	</ul>
	{#if chyba}<span class="w-sub" style:color="var(--danger)">{chyba}</span>{/if}
{/if}

<style>
	.cisla {
		display: flex;
		flex-wrap: wrap;
		gap: 6px 16px;
	}
	.c {
		display: flex;
		flex-direction: column;
		align-items: flex-start;
		gap: 1px;
		border: 0;
		background: none;
		padding: 0;
		color: var(--text);
		font: inherit;
		cursor: pointer;
	}
	.c b {
		font-size: 1.3rem;
		font-weight: 500;
	}
	.c:hover b {
		color: var(--ok);
	}
</style>
