<script>
	// Programs (v4, SPEC kap. 5): inventář aplikací + mapa souborů.
	// Seznam vlevo, detail s mapou vpravo. Každá cesta nese zdroj +
	// confidence — guess je tečkovaně (nikdy netvrdit, co jen hádáme).
	import { onMount } from 'svelte';
	import { goto } from '$app/navigation';
	import { invoke } from '@tauri-apps/api/core';
	import {
		RefreshCw,
		Search,
		HardDrive,
		Database,
		Settings2,
		Archive,
		ScrollText,
		BookKey,
		Scale,
		ExternalLink,
		ShieldCheck,
		PackageX
	} from 'lucide-svelte';
	import SystemBadge from '$lib/SystemBadge.svelte';
	import { isSystemApp } from '$lib/mandatory.js';
	import AppIcon from '$lib/AppIcon.svelte';


	let apps = $state([]);
	let procs = $state([]);
	let filter = $state('');
	let segment = $state('all'); // all | desktop | msix | running
	let sortKey = $state('name'); // name | publisher | date | paths
	let selected = $state(null);
	let map = $state([]);
	let sizing = $state(false);
	let loadError = $state('');

	// Ikony aplikací — stejný mechanismus jako Tasks (RGBA → canvas URL).
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

	const roles = {
		install: { label: 'Instalace', icon: HardDrive },
		data: { label: 'Data', icon: Database },
		config: { label: 'Konfigurace', icon: Settings2 },
		cache: { label: 'Cache', icon: Archive },
		logs: { label: 'Logy', icon: ScrollText },
		registry: { label: 'Registry', icon: BookKey }
	};
	const sources = {
		msi: 'MSI',
		msix: 'MSIX',
		registry: 'Registry',
		heuristic: 'Heuristika'
	};

	// Počet běžících procesů per identity_key (spojení s živým během).
	let running = $derived.by(() => {
		const m = new Map();
		for (const p of procs) {
			m.set(p.identity_key, (m.get(p.identity_key) ?? 0) + 1);
		}
		return m;
	});

	// Segment (Programy = klasické desktop instalace, Aplikace = MSIX
	// ze Store) + textový filtr + řazení.
	let shown = $derived.by(() => {
		const f = filter.trim().toLowerCase();
		let list = apps.filter((a) => {
			if (segment === 'desktop' && a.kind !== 'desktop') return false;
			if (segment === 'msix' && a.kind !== 'msix') return false;
			if (segment === 'running' && !running.get(a.identity_key)) return false;
			if (
				f &&
				!a.display_name.toLowerCase().includes(f) &&
				!(a.publisher ?? '').toLowerCase().includes(f)
			)
				return false;
			return true;
		});
		const cmp = {
			name: (a, b) => a.display_name.localeCompare(b.display_name, 'cs'),
			publisher: (a, b) => (a.publisher ?? '￿').localeCompare(b.publisher ?? '￿', 'cs'),
			date: (a, b) => (b.install_ts ?? 0) - (a.install_ts ?? 0),
			paths: (a, b) => b.path_count - a.path_count
		}[sortKey];
		return list.toSorted(cmp);
	});

	// Seskupení seznamu do sekcí s hlavičkami — ať se v 500 položkách
	// dá orientovat: dle názvu → písmena, dle vydavatele → vydavatel,
	// dle instalace → měsíce.
	let groupedList = $derived.by(() => {
		const label = (a) => {
			if (sortKey === 'publisher') return a.publisher ?? 'Bez vydavatele';
			if (sortKey === 'date') {
				if (!a.install_ts) return 'Neznámé datum';
				const d = new Date(a.install_ts * 1000);
				return d.toLocaleDateString('cs-CZ', { month: 'long', year: 'numeric' });
			}
			if (sortKey === 'paths') return null;
			const c = a.display_name[0]?.toUpperCase() ?? '#';
			return /[A-ZÁ-Ž]/.test(c) ? c : '#';
		};
		const groups = [];
		let current = null;
		for (const a of shown) {
			const l = label(a);
			if (l === null) {
				if (!current) groups.push((current = { label: null, items: [] }));
				current.items.push(a);
				continue;
			}
			if (!current || current.label !== l) {
				groups.push((current = { label: l, items: [] }));
			}
			current.items.push(a);
		}
		return groups;
	});

	let counts = $derived.by(() => ({
		desktop: apps.filter((a) => a.kind === 'desktop').length,
		msix: apps.filter((a) => a.kind === 'msix').length,
		running: apps.filter((a) => running.get(a.identity_key)).length
	}));

	function fmtSize(b) {
		if (b == null) return '—';
		if (b >= 1e9) return (b / 1e9).toFixed(1) + ' GB';
		if (b >= 1e6) return (b / 1e6).toFixed(1) + ' MB';
		if (b >= 1e3) return (b / 1e3).toFixed(0) + ' kB';
		return b + ' B';
	}

	function fmtDate(ts) {
		if (!ts) return '—';
		return new Date(ts * 1000).toLocaleDateString('cs-CZ');
	}

	async function load() {
		try {
			apps = await invoke('query_apps');
			loadError = '';
			for (const a of apps.slice(0, 400)) fetchIcon(a.identity_key);
		} catch (e) {
			loadError = String(e);
		}
		try {
			procs = await invoke('query_procs');
		} catch {
			procs = [];
		}
	}

	async function select(a) {
		selected = a;
		map = [];
		try {
			map = await invoke('query_app_map', { identityKey: a.identity_key });
		} catch {
			map = [];
		}
		// Velikosti rovnou a samy — cache z DB je hned, chybějící nebo
		// starší než den se dopočítají na pozadí (loader v řádcích).
		const dayAgo = Date.now() / 1000 - 86400;
		const stale = map.some(
			(p) => p.role !== 'registry' && (p.size_bytes == null || (p.size_ts ?? 0) < dayAgo)
		);
		if (stale) computeSizes(a.identity_key);
	}

	async function computeSizes(key) {
		if (sizing) return;
		sizing = true;
		try {
			const fresh = await invoke('compute_app_sizes', { identityKey: key });
			// Uživatel mohl mezitím kliknout jinam — nepřepsat cizí mapu.
			if (selected?.identity_key === key) map = fresh;
		} catch {
			/* chyba se ukáže absencí velikostí */
		}
		sizing = false;
	}

	async function rescan() {
		try {
			await invoke('rescan_apps');
		} catch {
			/* služba mimo */
		}
	}

	async function openPath(path) {
		try {
			await invoke('open_path', { path });
		} catch {
			/* registry větev / neexistující cesta */
		}
	}

	// ── Odinstalace (v8, SPEC 5.3): oficiální odinstalátor přes
	// validační vrstvu, po něm seznam zbytků k ruční revizi.
	let uninstPlan = $state(null); // { plan | deny, app }
	let uninstBusy = $state(false);
	let uninstToast = $state(null);
	let uninstRunning = $state(null); // app, jejíž odinstalátor běží
	let leftovers = $state(null); // { app, paths } po dokončení

	async function askUninstall(app) {
		uninstBusy = true;
		try {
			const r = await invoke('plan_uninstall', { identityKey: app.identity_key });
			uninstPlan = r.plan_id != null ? { plan: r, app } : { deny: r, app };
		} catch (e) {
			uninstToast = { kind: 'deny', text: String(e) };
		}
		uninstBusy = false;
	}

	// Odinstalátor spouští UI proces (běží pod tebou, ve tvé relaci) —
	// ne služba, která je jako SYSTEM v session 0 bez viditelné plochy.
	// Služba plán jen znovu zvaliduje, vydá příkaz a zapíše audit.
	// Čeká se, dokud odinstalátor neskončí — dialogy odklikáváš ty.
	async function confirmUninstall() {
		if (!uninstPlan?.plan || uninstBusy) return;
		const app = uninstPlan.app;
		const planId = uninstPlan.plan.plan_id;
		uninstBusy = true;
		uninstPlan = null;
		uninstRunning = app;
		try {
			const r = await invoke('run_uninstall', { planId, identityKey: app.identity_key });
			if (r.verdict === 'allow' && r.outcome === 'ok') {
				uninstToast = { kind: 'ok', text: `${app.display_name} odinstalováno` };
				// Co po aplikaci zbylo — ukázat, ne mazat.
				try {
					const paths = await invoke('query_leftovers', { identityKey: app.identity_key });
					if (paths.length) leftovers = { app, paths };
				} catch {
					/* zbytky se nepodařilo zjistit */
				}
				load();
			} else if (r.verdict === 'deny') {
				uninstToast = { kind: 'deny', text: r.deny_reason ?? 'odinstalace zamítnuta' };
			} else {
				// Odinstalátor doběhl, ale položka v registru zůstala —
				// nejčastěji ho uživatel zavřel/zrušil.
				uninstToast = {
					kind: 'deny',
					text: `${app.display_name} je pořád nainstalovaná — odinstalace nedoběhla do konce`
				};
				load();
			}
		} catch (e) {
			uninstToast = { kind: 'deny', text: String(e) };
		}
		uninstRunning = null;
		uninstBusy = false;
		setTimeout(() => (uninstToast = null), 6000);
	}

	// „x procesů běží" → Tasks se zaskrolováním a probliknutím řádku.
	function gotoRunning(key) {
		goto('/tasks?hl=' + encodeURIComponent(key));
	}

	let totalSize = $derived.by(() => {
		let t = 0;
		let any = false;
		for (const p of map) {
			if (p.size_bytes != null && p.role !== 'registry') {
				t += p.size_bytes;
				any = true;
			}
		}
		return any ? t : null;
	});

	onMount(() => {
		load();
		const t = setInterval(load, 30000);
		return () => clearInterval(t);
	});
</script>

<div class="page">
	<header class="head">
		<h1>Programs</h1>
		<div class="seg">
			<button class:active={segment === 'all'} onclick={() => (segment = 'all')}>
				Vše <i>{apps.length}</i>
			</button>
			<button class:active={segment === 'desktop'} onclick={() => (segment = 'desktop')}>
				Programy <i>{counts.desktop}</i>
			</button>
			<button class:active={segment === 'msix'} onclick={() => (segment = 'msix')}>
				Aplikace <i>{counts.msix}</i>
			</button>
			<button class:active={segment === 'running'} onclick={() => (segment = 'running')}>
				Běžící <i>{counts.running}</i>
			</button>
		</div>
		<select class="sort" bind:value={sortKey} title="Řazení">
			<option value="name">dle názvu</option>
			<option value="publisher">dle vydavatele</option>
			<option value="date">dle instalace</option>
			<option value="paths">dle počtu cest</option>
		</select>
		<div class="filter">
			<Search size={13} />
			<input placeholder="hledat aplikaci…" bind:value={filter} />
		</div>
		<button class="refresh" onclick={rescan} title="Přeskenovat inventář">
			<RefreshCw size={14} />
		</button>
	</header>

	{#if loadError}
		<p class="empty">Nelze načíst inventář: {loadError}</p>
	{:else}
		<div class="cols">
			<ul class="list">
				{#each groupedList as g (g.label ?? '·')}
					{#if g.label}
						<li class="grp-head label-tech">{g.label}</li>
					{/if}
					{#each g.items as a (a.identity_key)}
						{@const run = running.get(a.identity_key) ?? 0}
						<li>
							<button
								class="row"
								class:active={selected?.identity_key === a.identity_key}
								onclick={() => select(a)}
							>
								<AppIcon src={iconUrls[a.identity_key]} name={a.display_name} size={18} />
								<span class="row-main">
									<span class="row-title">
										{a.display_name}
										{#if isSystemApp(a)}<SystemBadge compact />{/if}
										{#if a.missing_install}
											<span
												class="ghost-badge"
												title="Instalační složka na disku neexistuje — po aplikaci zbyl jen záznam"
											>
												<PackageX size={13} /> chybí
											</span>
										{/if}
									</span>
									<span class="row-pub">{a.publisher ?? '—'}</span>
								</span>
								{#if run > 0}
									<span class="run-dot" title="{run} běžících procesů">{run}</span>
								{/if}
								<span class="row-ver mono">{a.version ?? ''}</span>
							</button>
						</li>
					{/each}
				{/each}
			</ul>

			<section class="detail">
				{#if !selected}
					<p class="empty">Vyber aplikaci vlevo — uvidíš, kde všude na disku žije.</p>
				{:else}
					<div class="d-head">
						<AppIcon src={iconUrls[selected.identity_key]} name={selected.display_name} size={30} />
						<div class="d-title">
							<h2>
								{selected.display_name}
								{#if isSystemApp(selected)}<SystemBadge />{/if}
								{#if selected.missing_install}
									<span class="ghost-badge big">
										<PackageX size={15} /> instalace chybí na disku
									</span>
								{/if}
							</h2>
							<span class="d-meta">
								{selected.publisher ?? '—'} · {selected.version ?? '—'} ·
								instalace {fmtDate(selected.install_ts)}
								{#if running.get(selected.identity_key)}
									· <button
										class="run-link"
										onclick={() => gotoRunning(selected.identity_key)}
										title="Ukázat v Tasks"
									>
										{running.get(selected.identity_key)} procesů běží
										<ExternalLink size={11} />
									</button>
								{/if}
							</span>
						</div>
						<span class="size-btn" class:loading={sizing}>
							<Scale size={14} />
							{sizing ? 'počítám velikosti…' : totalSize != null ? fmtSize(totalSize) : '—'}
						</span>
						{#if selected.kind === 'desktop' && !isSystemApp(selected)}
							<button
								class="uninst-btn"
								disabled={uninstBusy}
								title="Spustí oficiální odinstalátor aplikace"
								onclick={() => askUninstall(selected)}
							>
								<PackageX size={14} /> Odinstalovat
							</button>
						{/if}
					</div>

					{#if map.length === 0}
						<p class="empty">Mapa souborů je prázdná (portable aplikace bez stop?).</p>
					{:else}
						<ul class="map">
							{#each map as p (p.path)}
								{@const r = roles[p.role] ?? roles.data}
								<li class="map-row" class:guess={p.confidence === 'guess'}>
									<span class="m-role"><r.icon size={14} /> {r.label}</span>
									{#if p.role === 'registry'}
										<span class="m-path mono" title={p.path}>{p.path}</span>
									{:else}
										<button
											class="m-path mono m-link"
											title="Otevřít v Průzkumníku"
											onclick={() => openPath(p.path)}>{p.path}</button
										>
									{/if}
									<span class="m-src" data-conf={p.confidence}>
										{sources[p.source] ?? p.source}
									</span>
									<span class="m-size mono">
										{#if p.role === 'registry'}{''}
										{:else if p.size_bytes == null && sizing}<span class="m-load">···</span>
										{:else}{fmtSize(p.size_bytes)}{/if}
									</span>
								</li>
							{/each}
						</ul>
						<p class="legend">
							štítek = zdroj informace · <span class="lg-guess">tečkovaně</span> = odhad
							(heuristika) — MSI/MSIX je jistota, registry vysoká jistota
						</p>
					{/if}
				{/if}
			</section>
		</div>
	{/if}

	<!-- ── Odinstalace: plán → potvrzení (v8, T1) ── -->
	{#if uninstPlan}
		<div class="dlg-backdrop" role="presentation" onclick={() => (uninstPlan = null)} onkeydown={() => {}}>
			<div class="dlg" role="dialog" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
				{#if uninstPlan.deny}
					<h2>Nelze odinstalovat</h2>
					<p class="d-why">{uninstPlan.deny.deny_reason}</p>
					<div class="d-actions">
						<button class="d-btn" onclick={() => (uninstPlan = null)}>Zavřít</button>
					</div>
				{:else}
					<h2>Odinstalovat {uninstPlan.app.display_name}?</h2>
					<ul class="d-steps">
						{#each uninstPlan.plan.steps as s, i (i)}
							<li>{s.description}</li>
						{/each}
					</ul>
					<p class="d-note">
						Odinstalátor je od výrobce aplikace — jeho okno a dialogy jsou jeho, ne naše.
						Otevře se pod tvým účtem, stejně jako když ho spustíš z Ovládacích panelů;
						může se zeptat na oprávnění správce. Winsent po něm jen zkontroluje, co zbylo.
					</p>
					<div class="d-actions">
						<button class="d-btn" onclick={() => (uninstPlan = null)}>Zrušit</button>
						<button class="d-btn primary" disabled={uninstBusy} onclick={confirmUninstall}>
							{uninstBusy ? 'odinstalovávám…' : 'Spustit odinstalátor'}
						</button>
					</div>
				{/if}
			</div>
		</div>
	{/if}

	<!-- ── Běžící odinstalátor: okno má uživatel, my jen čekáme ── -->
	{#if uninstRunning}
		<div class="dlg-backdrop" role="presentation">
			<div class="dlg" role="dialog">
				<h2>Odinstalovávám {uninstRunning.display_name}</h2>
				<p class="d-note">
					Odinstalátor běží ve vlastním okně — dokonči ho tam. Když se okno neukázalo,
					hledej ho na hlavním panelu; může čekat na potvrzení oprávnění správce.
				</p>
				<p class="d-why">Winsent počká, až skončí, a pak ověří, co po něm zbylo.</p>
			</div>
		</div>
	{/if}

	<!-- ── Zbytky po odinstalaci: ukázat, nemazat ── -->
	{#if leftovers}
		<div class="dlg-backdrop" role="presentation" onclick={() => (leftovers = null)} onkeydown={() => {}}>
			<div class="dlg" role="dialog" onclick={(e) => e.stopPropagation()} onkeydown={() => {}}>
				<h2>Po odinstalaci zbylo {leftovers.paths.length} položek</h2>
				<p class="d-note">
					Odinstalátor je nechal na disku. Často jsou to tvoje data — profily, nastavení,
					uložené pozice ve hře. Projdi si je a smaž jen to, co opravdu nechceš.
				</p>
				<ul class="d-steps">
					{#each leftovers.paths as p (p)}
						<li>
							<button class="left-path mono" onclick={() => openPath(p)} title="Otevřít v Průzkumníku"
								>{p}</button
							>
						</li>
					{/each}
				</ul>
				<div class="d-actions">
					<button class="d-btn" onclick={() => (leftovers = null)}>Zavřít</button>
				</div>
			</div>
		</div>
	{/if}
	{#if uninstToast}
		<div class="dlg-toast {uninstToast.kind}">{uninstToast.text}</div>
	{/if}
</div>

<style>
	.page {
		display: flex;
		flex-direction: column;
		gap: 14px;
		height: 100%;
		min-height: 0;
	}
	.head {
		display: flex;
		align-items: center;
		gap: 12px;
	}
	.head h1 {
		font-size: 1.15rem;
		font-weight: 600;
		letter-spacing: 0.02em;
	}
	.sub {
		color: var(--text-faint);
		font-size: 0.78rem;
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
	.refresh {
		background: none;
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		padding: 5px 7px;
		cursor: pointer;
		display: grid;
		place-items: center;
	}
	.refresh:hover {
		color: var(--text);
		border-color: var(--border-strong);
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
	/* Select ve stejném jazyce jako segmenty — tmavý, bez nativního vzhledu. */
	.sort {
		appearance: none;
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text-dim);
		font: inherit;
		font-size: 0.8rem;
		padding: 6px 26px 6px 10px;
		cursor: pointer;
		color-scheme: dark;
		background-image: url("data:image/svg+xml,%3Csvg xmlns='http://www.w3.org/2000/svg' width='10' height='6'%3E%3Cpath d='M1 1l4 4 4-4' stroke='%239a9aa1' fill='none' stroke-width='1.5'/%3E%3C/svg%3E");
		background-repeat: no-repeat;
		background-position: right 9px center;
	}
	.sort:hover {
		color: var(--text);
		border-color: var(--border-strong);
	}
	/* Rozbalený seznam — WebView2 jinak kreslí bílé pozadí. */
	.sort option {
		background-color: #16171c;
		color: var(--text);
	}
	/* Aplikace, po které zbyl jen záznam — instalační složka je pryč. */
	/* Odinstalace + dialogy (stejný jazyk jako Files/Tasks) */
	.uninst-btn {
		display: flex;
		align-items: center;
		gap: 6px;
		background: var(--panel);
		border: 1px solid color-mix(in srgb, var(--warn) 40%, var(--border));
		border-radius: var(--radius-sm);
		color: var(--warn);
		font: inherit;
		font-size: 0.78rem;
		padding: 6px 12px;
		cursor: pointer;
		white-space: nowrap;
	}
	.uninst-btn:hover {
		border-color: var(--warn);
	}
	.uninst-btn:disabled {
		opacity: 0.6;
		cursor: wait;
	}
	.dlg-backdrop {
		position: fixed;
		inset: 0;
		background: rgba(8, 9, 11, 0.62);
		display: grid;
		place-items: center;
		z-index: 40;
	}
	.dlg {
		width: min(620px, 92vw);
		background: #16171c;
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-lg);
		padding: 20px 22px;
	}
	.dlg h2 {
		font-size: 1.05rem;
		margin-bottom: 10px;
	}
	.d-steps {
		list-style: none;
		margin: 0 0 12px;
		padding: 0;
		max-height: 42vh;
		overflow-y: auto;
		font-size: 0.84rem;
	}
	.d-steps li {
		padding: 5px 0;
		border-bottom: 1px dashed var(--border);
		word-break: break-all;
	}
	.left-path {
		background: none;
		border: none;
		color: var(--text);
		font: inherit;
		font-size: 0.8rem;
		text-align: left;
		cursor: pointer;
		padding: 0;
		word-break: break-all;
	}
	.left-path:hover {
		color: var(--accent);
		text-decoration: underline;
	}
	.d-why {
		color: var(--danger);
		font-size: 0.88rem;
		margin-bottom: 12px;
	}
	.d-note {
		font-size: 0.8rem;
		color: var(--text-faint);
		margin-bottom: 14px;
		line-height: 1.5;
	}
	.d-actions {
		display: flex;
		justify-content: flex-end;
		gap: 8px;
	}
	.d-btn {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text);
		font: inherit;
		font-size: 0.84rem;
		padding: 7px 14px;
		cursor: pointer;
	}
	.d-btn.primary {
		border-color: color-mix(in srgb, var(--warn) 50%, transparent);
		color: var(--warn);
	}
	.dlg-toast {
		position: fixed;
		bottom: 18px;
		left: 50%;
		transform: translateX(-50%);
		padding: 9px 16px;
		border-radius: var(--radius);
		background: #16171c;
		border: 1px solid var(--border-strong);
		font-size: 0.84rem;
		z-index: 50;
	}
	.dlg-toast.ok {
		color: var(--ok);
	}
	.dlg-toast.deny {
		color: var(--danger);
	}

	.ghost-badge {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		font-size: 0.68rem;
		color: var(--warn);
		border: 1px dotted color-mix(in srgb, var(--warn) 55%, transparent);
		border-radius: 999px;
		padding: 1px 7px;
		margin-left: 5px;
		vertical-align: -1px;
		flex: none;
	}
	.ghost-badge.big {
		font-size: 0.74rem;
		padding: 2px 9px;
	}

	.mand {
		display: inline-flex;
		align-items: center;
		gap: 4px;
		color: var(--net-down);
		vertical-align: -2px;
		margin-left: 4px;
	}
	.mand.big {
		font-size: 0.7rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		border: 1px solid color-mix(in srgb, var(--net-down) 40%, transparent);
		border-radius: 999px;
		padding: 2px 8px;
		margin-left: 8px;
	}
	.grp-head {
		position: sticky;
		top: 0;
		background: rgba(22, 23, 28, 0.92);
		padding: 5px 12px 4px;
		font-size: 0.68rem;
		color: var(--text-faint);
		border-bottom: 1px dashed var(--border);
		z-index: 1;
	}
	.m-load {
		color: var(--text-faint);
		animation: pulse 1s ease infinite;
	}
	@keyframes pulse {
		50% {
			opacity: 0.35;
		}
	}
	.size-btn.loading {
		color: var(--text-dim);
		animation: pulse 1.2s ease infinite;
	}

	.cols {
		display: grid;
		grid-template-columns: minmax(380px, 500px) 1fr;
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
		width: 100%;
		display: flex;
		align-items: center;
		gap: 10px;
		padding: 7px 12px;
		background: none;
		border: none;
		border-bottom: 1px dashed var(--border);
		color: var(--text);
		cursor: pointer;
		text-align: left;
		font: inherit;
	}
	.row:hover {
		background: var(--surface-hover);
	}
	.row.active {
		background: var(--surface-hover);
		box-shadow: inset 2px 0 0 var(--accent);
	}
	.app-icon {
		width: 18px;
		height: 18px;
		flex: none;
		border-radius: 3px;
	}
	.app-icon.big {
		width: 30px;
		height: 30px;
	}
	.app-icon.ph {
		background: var(--surface-hover);
		border: 1px dashed var(--border);
		display: inline-block;
	}
	.row-main {
		display: flex;
		flex-direction: column;
		min-width: 0;
		flex: 1;
	}
	.row-title {
		font-size: 0.84rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row-pub {
		font-size: 0.7rem;
		color: var(--text-faint);
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	.row-ver {
		font-size: 0.66rem;
		color: var(--text-faint);
		max-width: 76px;
		overflow: hidden;
		text-overflow: ellipsis;
		white-space: nowrap;
	}
	.run-dot {
		font-family: var(--font-mono);
		font-size: 0.66rem;
		color: var(--ok);
		border: 1px solid color-mix(in srgb, var(--ok) 45%, transparent);
		border-radius: 999px;
		padding: 0 6px;
		text-shadow: var(--glow-ok);
	}

	.detail {
		border: 1px dashed var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		padding: 16px;
		overflow-y: auto;
		min-height: 0;
	}
	.d-head {
		display: flex;
		align-items: center;
		gap: 12px;
		margin-bottom: 14px;
	}
	.d-title {
		min-width: 0;
		flex: 1;
	}
	.d-head h2 {
		font-size: 1.15rem;
		font-weight: 600;
	}
	.d-meta {
		font-size: 0.82rem;
		color: var(--text-dim);
	}
	.run-link {
		background: none;
		border: none;
		padding: 0;
		font: inherit;
		font-size: 0.82rem;
		color: var(--ok);
		cursor: pointer;
		display: inline-flex;
		align-items: center;
		gap: 4px;
		text-shadow: var(--glow-ok);
	}
	.run-link:hover {
		text-decoration: underline;
	}
	.size-btn {
		display: flex;
		align-items: center;
		gap: 6px;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		color: var(--text);
		font: inherit;
		font-size: 0.76rem;
		padding: 6px 10px;
		cursor: pointer;
		white-space: nowrap;
	}
	.size-btn:hover {
		border-color: var(--border-strong);
	}
	.size-btn:disabled {
		opacity: 0.6;
		cursor: wait;
	}

	.map {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 4px;
	}
	.map-row {
		display: grid;
		grid-template-columns: 134px 1fr auto 96px;
		gap: 12px;
		align-items: center;
		padding: 9px 12px;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-sm);
		font-size: 0.88rem;
	}
	.map-row.guess .m-path {
		text-decoration: underline dotted var(--text-faint);
		text-underline-offset: 3px;
	}
	.m-role {
		display: flex;
		align-items: center;
		gap: 7px;
		color: var(--text-dim);
		font-size: 0.8rem;
	}
	.m-path {
		font-size: 0.8rem;
		white-space: nowrap;
		overflow: hidden;
		text-overflow: ellipsis;
		direction: rtl;
		text-align: left;
	}
	.m-link {
		background: none;
		border: none;
		padding: 0;
		color: var(--text);
		cursor: pointer;
		min-width: 0;
	}
	.m-link:hover {
		color: var(--accent);
		text-decoration: underline;
		text-underline-offset: 3px;
	}
	.m-src {
		font-size: 0.66rem;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		padding: 2px 7px;
		border-radius: 999px;
		border: 1px solid var(--border-strong);
		color: var(--text-dim);
	}
	.m-src[data-conf='exact'] {
		color: var(--ok);
		border-color: color-mix(in srgb, var(--ok) 40%, transparent);
	}
	.m-src[data-conf='guess'] {
		border-style: dotted;
		color: var(--text-faint);
	}
	.m-size {
		font-size: 0.8rem;
		text-align: right;
		color: var(--text-dim);
	}
	.legend {
		margin-top: 10px;
		font-size: 0.72rem;
		color: var(--text-faint);
	}
	.lg-guess {
		text-decoration: underline dotted;
		text-underline-offset: 3px;
	}
	.mono {
		font-family: var(--font-mono);
	}
	.empty {
		color: var(--text-faint);
		font-size: 0.85rem;
		padding: 18px;
	}
</style>
