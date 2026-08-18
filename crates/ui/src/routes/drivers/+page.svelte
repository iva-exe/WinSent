<script>
	// Drivers (v10, SPEC kap. 6): co v počítači běží za ovladače, od koho
	// to je a jak je to staré.
	//
	// Sekce jen čte. Instalovat ovladače Winsent neumí a nebude — od toho
	// je Windows Update, který to má vyzkoušené a umí se vrátit zpátky.
	// Ukázat, který ovladač je z roku 2015 a od koho, je práce, kterou
	// Správce zařízení dělá mizerně a která nikoho nemůže rozbít.
	import { onMount } from 'svelte';
	import { invoke } from '@tauri-apps/api/core';
	import { Search, Package, PackageCheck, TriangleAlert, Calendar } from 'lucide-svelte';

	let report = $state(null);
	let loadError = $state('');
	let filter = $state('');
	// vše | oem (doinstalované) | old (staré) | problem
	let segment = $state('all');

	async function load() {
		try {
			report = await invoke('query_drivers');
			loadError = '';
		} catch (e) {
			loadError = String(e);
		}
	}

	onMount(() => {
		load();
		// Ovladače se mění leda novým hardwarem; služba je navíc drží
		// pět minut v cache.
		const t = setInterval(load, 300000);
		return () => clearInterval(t);
	});

	// Rok z data ovladače. Systém ho hlásí v místním formátu, takže se
	// hledá ten díl, který na rok vypadá — nic jiného z data nepotřebujeme.
	function yearOf(d) {
		for (const n of (d.date ?? '').split(/\D+/)) {
			const v = Number(n);
			if (v > 1900 && v < 2200) return v;
		}
		return null;
	}

	const thisYear = new Date().getFullYear();
	// „Starý" má smysl jen u ovladačů od výrobců.
	//
	// Windows dávají VŠEM svým vestavěným ovladačům jedno a totéž datum
	// (21. 6. 2006) bez ohledu na to, kdy vznikly — naměřeno na tomhle
	// stroji u stovky z nich. Kdyby se počítaly, byl by seznam „starých"
	// zaplavený ovladači, se kterými není nic špatně, a ten jeden
	// skutečně zastaralý od výrobce by v tom zapadl.
	const OLD_YEARS = 5;
	function isOld(d) {
		if (!d.third_party) return false;
		const y = yearOf(d);
		return y != null && thisYear - y >= OLD_YEARS;
	}

	let shown = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		return (report?.drivers ?? []).filter((d) => {
			if (segment === 'oem' && !d.third_party) return false;
			if (segment === 'old' && !isOld(d)) return false;
			if (segment === 'problem' && !d.problem_code) return false;
			if (!q) return true;
			return `${d.device} ${d.provider} ${d.class_desc} ${d.inf} ${d.version}`
				.toLowerCase()
				.includes(q);
		});
	});

	let counts = $derived({
		all: report?.drivers?.length ?? 0,
		oem: report?.third_party ?? 0,
		old: (report?.drivers ?? []).filter(isOld).length,
		problem: report?.with_problem ?? 0
	});
</script>

<div class="page">
	<header class="head">
		<h1>Drivers</h1>
		<div class="seg">
			<button class:active={segment === 'all'} onclick={() => (segment = 'all')}>
				Vše <i>{counts.all}</i>
			</button>
			<button class:active={segment === 'oem'} onclick={() => (segment = 'oem')}>
				Od výrobců <i>{counts.oem}</i>
			</button>
			<button class:active={segment === 'old'} onclick={() => (segment = 'old')}>
				Zastaralé od výrobců <i>{counts.old}</i>
			</button>
			<button class:active={segment === 'problem'} onclick={() => (segment = 'problem')}>
				S problémem <i>{counts.problem}</i>
			</button>
		</div>
		<div class="filter">
			<Search size={14} />
			<input placeholder="hledat ovladač…" bind:value={filter} />
		</div>
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst ovladače: {loadError}</p>
	{:else if report}
		<div class="body">
			{#if !shown.length}
				<p class="empty">
					{filter.trim() ? 'Nic neodpovídá hledání.' : 'V tomhle zobrazení nic není.'}
				</p>
			{/if}
			{#each shown as d, i (i + ':' + d.group_key)}
				{@const year = yearOf(d)}
				<article class="item" class:bad={d.problem_code}>
					<div class="ico">
						{#if d.problem_code}
							<TriangleAlert size={19} />
						{:else if d.third_party}
							<Package size={19} />
						{:else}
							<PackageCheck size={19} />
						{/if}
					</div>
					<div class="info">
						<h3>{d.device}</h3>
						<p class="vendor">{d.provider || '—'}</p>
						<div class="facts">
							{#if d.version}<span class="fact">verze {d.version}</span>{/if}
							{#if d.date}
								<span class="fact" class:old={isOld(d)}>
									<Calendar size={12} />
									{d.date}
									{#if isOld(d) && year}· {thisYear - year} let starý{/if}
								</span>
							{/if}
							{#if d.class_desc}<span class="fact muted">{d.class_desc}</span>{/if}
							{#if d.inf}<span class="fact mono muted">{d.inf}</span>{/if}
						</div>
					</div>
					<div class="side">
						{#if d.problem_code}
							<span class="pill bad">problém {d.problem_code}</span>
						{:else if d.third_party}
							<!-- Doinstalovaný ovladač není nic špatného — je to
							     obvykle ten správný od výrobce. Jen se hodí vědět,
							     že nepřišel s Windows. -->
							<span class="pill">od výrobce</span>
						{:else}
							<span class="pill quiet">z Windows</span>
						{/if}
					</div>
				</article>
			{/each}

			<p class="note">
				Winsent ovladače jen čte. Instalovat ani vracet je neumí — od toho je Windows Update,
				který to má vyzkoušené na milionech strojů a v případě problému se umí vrátit zpátky.
				„Od výrobce" znamená, že ovladač doinstaloval někdo zvenčí (soubor <code>oem*.inf</code>);
				u grafiky, zvukovky nebo síťovky je to obvyklé a správné. Stáří je zvednutý prst,
				ne poplach: ovladač čipsetu z roku 2019 může být poslední, který kdy vyšel. Počítá se jen u ovladačů od výrobců — Windows dávají všem svým vestavěným jedno a totéž datum 21. 6. 2006 bez ohledu na to, kdy vznikly.
			</p>
		</div>
	{:else}
		<p class="empty">Načítám ovladače…</p>
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
	.seg {
		display: flex;
		gap: 2px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 2px;
		background: var(--surface);
	}
	.seg button {
		background: none;
		border: none;
		color: var(--text-dim);
		font: inherit;
		font-size: 0.76rem;
		padding: 4px 10px;
		border-radius: 3px;
		cursor: pointer;
		display: flex;
		align-items: center;
		gap: 6px;
	}
	.seg button i {
		font-style: normal;
		font-family: var(--font-mono);
		font-size: 0.64rem;
		color: var(--text-faint);
	}
	.seg button.active {
		background: var(--surface-hover);
		color: var(--text);
		box-shadow: inset 0 0 0 1px var(--border-strong);
	}
	.filter {
		margin-left: auto;
		display: flex;
		align-items: center;
		gap: 6px;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		padding: 4px 8px;
		color: var(--text-dim);
		background: var(--surface);
	}
	.filter input {
		background: none;
		border: none;
		outline: none;
		color: var(--text);
		font: inherit;
		font-size: 0.8rem;
		width: 170px;
	}
	.body {
		flex: 1;
		min-height: 0;
		overflow-y: auto;
		padding-right: 6px;
	}
	.item {
		display: flex;
		align-items: flex-start;
		gap: 12px;
		padding: 11px 13px;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		margin-bottom: 7px;
	}
	.item.bad {
		border-color: color-mix(in srgb, var(--danger) 45%, var(--border));
	}
	.ico {
		color: var(--text-dim);
		display: flex;
		padding-top: 2px;
	}
	.item.bad .ico {
		color: var(--danger);
	}
	.info {
		flex: 1;
		min-width: 0;
	}
	.info h3 {
		margin: 0;
		font-size: 1.02rem;
		font-weight: 600;
		line-height: 1.3;
		word-break: break-word;
	}
	.vendor {
		margin: 3px 0 0;
		font-size: 0.8rem;
		color: var(--text-dim);
	}
	.facts {
		display: flex;
		flex-wrap: wrap;
		gap: 6px 10px;
		margin-top: 6px;
	}
	.fact {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		font-size: 0.74rem;
		color: var(--text-dim);
	}
	.fact.muted {
		color: var(--text-faint);
	}
	.fact.mono {
		font-family: var(--font-mono);
		font-size: 0.66rem;
	}
	/* Stáří je informace, ne chyba — jantarová, ne červená. */
	.fact.old {
		color: var(--warn);
	}
	.side {
		flex-shrink: 0;
	}
	.pill {
		display: inline-flex;
		align-items: center;
		gap: 5px;
		padding: 3px 10px;
		border-radius: 999px;
		border: 1px solid var(--border);
		font-size: 0.74rem;
		white-space: nowrap;
		color: var(--text-dim);
	}
	.pill.quiet {
		color: var(--text-faint);
	}
	.pill.bad {
		color: var(--danger);
		border-color: color-mix(in srgb, var(--danger) 55%, transparent);
	}
	.note {
		margin: 14px 0 0;
		font-size: 0.78rem;
		line-height: 1.55;
		color: var(--text-faint);
	}
	.note code {
		font-family: var(--font-mono);
		font-size: 0.72rem;
	}
	.empty {
		color: var(--text-faint);
		font-size: 0.85rem;
	}
</style>
