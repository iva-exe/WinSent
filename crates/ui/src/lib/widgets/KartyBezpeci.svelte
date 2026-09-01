<script>
	// Dlaždice ze sekcí Security, Incidents, On start, Users a Historie.
	//
	// Jediná akce, která se odsud dělá, je přepnutí položky po startu —
	// vratná, oficiální cesta Windows a totéž, co dělá Správce úloh.
	// Nic se nemaže a nic se neodinstalovává: nevratné věci patří tam,
	// kde je vidět plán a co přesně zmizí.
	import { invoke } from '@tauri-apps/api/core';
	import AppIcon from '$lib/AppIcon.svelte';
	import { data } from './data.svelte.js';
	import { ikony, chciIkonu } from './ikony.svelte.js';
	import { ochranaRadky } from '$lib/protection.js';
	import { skryte, jeSkryty, klicIncidentu } from '$lib/hidden.svelte.js';
	import { pred, doba, pocet } from './pomoc.js';

	// Rozměry přicházejí v jednotkách mřížky: šířka ve sloupcích,
	// výška v řádcích (řádek je nízký, viz registr.js). Obsah se podle
	// nich rozhoduje, co se ještě vejde.
	let { typ, w = 1, h = 2 } = $props();

	let siroka = $derived(w >= 2);
	let velka = $derived(w >= 2 && h >= 4);

	// ── ochrana ──────────────────────────────────────────────────────
	let radky = $derived(ochranaRadky(data.security));
	let souhrnOchrany = $derived.by(() => {
		const c = { ok: 0, warn: 0, dim: 0 };
		for (const r of radky) c[r.tone] = (c[r.tone] ?? 0) + 1;
		return c;
	});

	// ── oprávnění ────────────────────────────────────────────────────
	const CAPS = {
		microphone: 'mikrofon',
		webcam: 'kamera',
		location: 'poloha'
	};
	let ziveOpravneni = $derived((data.security?.permissions ?? []).filter((p) => p.in_use));
	let naposledyOpravneni = $derived.by(() => {
		const rows = (data.security?.permissions ?? []).filter((p) => p.last_used);
		return rows.sort((a, b) => b.last_used - a.last_used)[0] ?? null;
	});
	function otevriSoukromi(cap) {
		invoke('open_settings_page', { page: `privacy-${cap}` }).catch(() => {});
	}

	// Součty za týden. Klíč historie je cesta k programu, čitelné jméno
	// zná až seznam oprávnění — proto se to spojuje tady a řádky bez
	// naměřeného času se zahazují (nula v žebříčku „jak dlouho" nic
	// neříká).
	let mikrofon = $derived.by(() => {
		const jmena = new Map();
		for (const p of data.security?.permissions ?? []) {
			jmena.set(p.app, { name: p.app_name, group: p.group_key });
		}
		const soucty = new Map();
		for (const [app, cap, secs] of data.permUse ?? []) {
			if (cap !== 'microphone' || !secs) continue;
			const info = jmena.get(app);
			const key = info?.group ?? app;
			const zaznam = soucty.get(key) ?? { name: info?.name ?? app.split(/[\\/]/).pop(), s: 0 };
			zaznam.s += secs;
			soucty.set(key, zaznam);
		}
		return [...soucty.values()].sort((a, b) => b.s - a.s).slice(0, velka ? 6 : 3);
	});

	// ── incidenty ────────────────────────────────────────────────────
	const DRUH = {
		stall: 'zásek systému',
		app_crash: 'pád aplikace',
		bsod: 'BSOD'
	};
	// Skryté incidenty se na přehled nedostanou — uživatel je schoval
	// právě proto, aby mu nesvítily. Že nějaké jsou, se ale zamlčet
	// nesmí: jinak by dlaždice tvrdila „žádný incident" u stroje,
	// který jich má deset.
	let incidenty = $derived((data.incidents ?? []).filter((i) => !jeSkryty(klicIncidentu(i.id))));
	let skryto = $derived((data.incidents ?? []).length - incidenty.length);
	let posledni = $derived(incidenty[0] ?? null);
	let zaMesic = $derived.by(() => {
		const od = Date.now() / 1000 - 30 * 86400;
		const c = {};
		for (const i of incidenty) {
			if (i.ts < od) continue;
			c[i.kind] = (c[i.kind] ?? 0) + 1;
		}
		return c;
	});
	let zaMesicCelkem = $derived(Object.values(zaMesic).reduce((a, b) => a + b, 0));

	// ── po startu ────────────────────────────────────────────────────
	let prepinatelne = $derived.by(() => {
		const rows = (data.startup ?? []).filter((r) => !r.system && r.toggleable);
		return [...rows]
			.sort((a, b) => Number(b.enabled) - Number(a.enabled) || (a.app_name ?? a.name).localeCompare(b.app_name ?? b.name, 'cs'))
			.slice(0, velka ? 7 : 4);
	});
	let startSouhrn = $derived.by(() => {
		const rows = data.startup ?? [];
		return {
			vse: rows.length,
			zapnute: rows.filter((r) => r.enabled).length,
			moje: rows.filter((r) => !r.system).length,
			sluzby: rows.filter((r) => r.source === 'service').length
		};
	});
	let prepinam = $state('');
	let chyba = $state('');
	async function prepni(r) {
		if (prepinam) return;
		prepinam = r.id;
		chyba = '';
		try {
			const v = await invoke('toggle_startup', { id: r.id, on: !r.enabled });
			if (v.verdict === 'deny') chyba = v.deny_reason ?? 'zamítnuto';
			else if (data.startup) {
				// Odpověď se promítne rovnou, ať přepínač nečeká pět
				// minut na další dotaz.
				data.startup = data.startup.map((x) => (x.id === r.id ? { ...x, enabled: !r.enabled } : x));
			}
		} catch (e) {
			chyba = String(e);
		}
		prepinam = '';
	}

	$effect(() => {
		for (const r of prepinatelne) if (r.identity_key) chciIkonu(r.identity_key);
	});

	// ── audit ────────────────────────────────────────────────────────
	let filtr = $state('vse');
	let auditRadky = $derived.by(() => {
		const rows = data.audit ?? [];
		const f = filtr === 'vse' ? rows : rows.filter((r) => r.verdict === filtr);
		return f.slice(0, velka ? 8 : 4);
	});

	// ── účty ─────────────────────────────────────────────────────────
	let ucty = $derived(data.users);
	let spravci = $derived((ucty?.users ?? []).filter((u) => u.admin && !u.disabled));
</script>

{#if typ === 'ochrana'}
	{#if radky.length}
		<span class="w-sub">
			{souhrnOchrany.ok} v pořádku · {souhrnOchrany.warn} k dořešení · {souhrnOchrany.dim ?? 0} nezjištěno
		</span>
		<ul class="w-list scroll">
			{#each radky.slice(0, velka ? 8 : 3) as r, i (i)}
				<li class="w-row">
					<r.icon size={14} color={r.tone === 'ok' ? 'var(--ok)' : r.tone === 'warn' ? 'var(--warn)' : 'var(--text-faint)'} />
					<span class="w-name">{r.name}</span>
					<span
						class="w-sub"
						style:color={r.tone === 'warn' ? 'var(--warn)' : 'var(--text-dim)'}>{r.state}</span
					>
				</li>
			{/each}
		</ul>
	{:else}
		<span class="w-empty">Stav ochrany se ještě nenačetl.</span>
	{/if}
{:else if typ === 'poslouchaji'}
	{#if ziveOpravneni.length}
		<ul class="w-list">
			{#each ziveOpravneni.slice(0, 4) as p (p.capability + p.app)}
				<li>
					<button class="w-klik w-row" onclick={() => otevriSoukromi(p.capability)}>
						<span class="ziva"></span>
						<span class="w-name">{p.app_name}</span>
						<span class="w-sub">{CAPS[p.capability] ?? p.capability}</span>
					</button>
				</li>
			{/each}
		</ul>
	{:else}
		<span class="w-mid">Nikdo neposlouchá.</span>
		{#if naposledyOpravneni}
			<span class="w-sub">
				naposledy {naposledyOpravneni.app_name} — {CAPS[naposledyOpravneni.capability] ??
					naposledyOpravneni.capability}, {pred(naposledyOpravneni.last_used)}
			</span>
		{/if}
	{/if}
{:else if typ === 'mikrofon'}
	{#if mikrofon.length}
		<ul class="w-list scroll">
			{#each mikrofon as m, i (i)}
				<li class="w-row">
					<span class="w-name">{m.name}</span>
					<span class="w-mono w-dim">{doba(m.s)}</span>
				</li>
			{/each}
		</ul>
		<span class="w-sub">součet za posledních 7 dní</span>
	{:else}
		<span class="w-empty">Za poslední týden mikrofon nikdo nedržel.</span>
	{/if}
{:else if typ === 'incident'}
	{#if posledni}
		<span class="w-mid" style:color={posledni.kind === 'bsod' ? 'var(--danger)' : 'var(--warn)'}>
			{DRUH[posledni.kind] ?? posledni.kind}
			{#if posledni.culprit}— {posledni.culprit}{/if}
		</span>
		<span class="w-sub">{pred(posledni.ts)}</span>
		{#if velka}
			<ul class="w-list scroll">
				{#each incidenty.slice(1, 6) as i (i.id)}
					<li class="w-row">
						<span class="w-name">{DRUH[i.kind] ?? i.kind}{#if i.culprit} — {i.culprit}{/if}</span>
						<span class="w-sub">{pred(i.ts)}</span>
					</li>
				{/each}
			</ul>
		{/if}
	{:else if skryto}
		<span class="w-mid">Nic nového.</span>
		<span class="w-sub">{skryto} incidentů máš skrytých — jsou v sekci Incidents</span>
	{:else}
		<span class="w-mid">Žádný incident.</span>
		<span class="w-sub">systém jede čistě</span>
	{/if}
{:else if typ === 'incidenty30'}
	<span class="w-big" style:color={zaMesicCelkem ? 'var(--warn)' : 'var(--text)'}>{zaMesicCelkem}</span>
	<span class="w-sub">za posledních 30 dní</span>
	{#if zaMesicCelkem}
		<span class="w-sub">
			{Object.entries(zaMesic)
				.map(([k, v]) => `${v}× ${DRUH[k] ?? k}`)
				.join(' · ')}
		</span>
	{/if}
{:else if typ === 'startup'}
	<ul class="w-list scroll">
		{#each prepinatelne as r (r.id)}
			<li class="w-row">
				<AppIcon src={ikony[r.identity_key]} name={r.app_name || r.name} size={15} />
				<span class="w-name" title={r.command}>{r.app_name || r.name}</span>
				<span class="w-sub">{r.source}</span>
				<button
					class="prep"
					class:on={r.enabled}
					disabled={prepinam === r.id}
					title={r.enabled ? 'Vypnout po startu' : 'Zapnout po startu'}
					onclick={() => prepni(r)}
				>
					<span class="knof"></span>
				</button>
			</li>
		{/each}
		{#if !prepinatelne.length}<li class="w-empty">Nic přepínatelného tu není.</li>{/if}
	</ul>
	{#if chyba}<span class="w-sub" style:color="var(--danger)">{chyba}</span>{/if}
{:else if typ === 'startpocet'}
	<span class="w-big">{startSouhrn.zapnute}</span>
	<span class="w-sub">položek startuje s Windows</span>
	<span class="w-sub">
		z toho {startSouhrn.moje} nepatří Windows{#if siroka} · {startSouhrn.sluzby} služeb{/if}
	</span>
{:else if typ === 'ucty'}
	{#if ucty}
		<span class="w-big">{ucty.users.length}</span>
		<span class="w-sub">
			{pocet(spravci.length, 'správce', 'správci', 'správců')}
			{#if ucty.foreign_admins?.length}· {ucty.foreign_admins.length} mimo lokální účty{/if}
		</span>
		{#if siroka}
			<span class="w-sub">přihlášen: {ucty.current_user}</span>
		{/if}
	{:else}
		<span class="w-empty">Účty se ještě nenačetly.</span>
	{/if}
{:else if typ === 'audit'}
	<div class="w-segs">
		<button class="w-seg" class:on={filtr === 'vse'} onclick={() => (filtr = 'vse')}>Vše</button>
		<button class="w-seg" class:on={filtr === 'allow'} onclick={() => (filtr = 'allow')}>Provedeno</button>
		<button class="w-seg" class:on={filtr === 'deny'} onclick={() => (filtr = 'deny')}>Zamítnuto</button>
	</div>
	<ul class="w-list scroll">
		{#each auditRadky as a (a.id)}
			<li class="w-row">
				<span
					class="tec"
					style:background={a.verdict === 'deny' ? 'var(--danger)' : 'var(--ok)'}
				></span>
				<span class="w-name" title={a.target}>{a.action}{#if a.target} · {a.target}{/if}</span>
				<span class="w-sub">{pred(a.ts)}</span>
			</li>
		{/each}
		{#if !auditRadky.length}<li class="w-empty">Zatím nic takového.</li>{/if}
	</ul>
{/if}

<style>
	.ziva {
		flex: none;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--danger);
		box-shadow: var(--glow-danger);
	}
	.tec {
		flex: none;
		width: 5px;
		height: 5px;
		border-radius: 50%;
	}
	/* Přepínač po startu: stejná mechanika jako v sekci On start —
	   vratná změna, žádné potvrzování. */
	.prep {
		flex: none;
		width: 28px;
		height: 16px;
		padding: 0;
		border: 1px solid var(--border-strong);
		border-radius: 8px;
		background: var(--surface-hover);
		cursor: pointer;
		display: flex;
		align-items: center;
	}
	.prep .knof {
		width: 10px;
		height: 10px;
		margin: 0 2px;
		border-radius: 50%;
		background: var(--text-faint);
		transition: transform 0.15s ease, background 0.15s ease;
	}
	.prep.on {
		background: color-mix(in srgb, var(--ok) 28%, transparent);
		border-color: color-mix(in srgb, var(--ok) 55%, transparent);
	}
	.prep.on .knof {
		transform: translateX(12px);
		background: var(--ok);
	}
	.prep:disabled {
		opacity: 0.5;
		cursor: default;
	}
</style>
