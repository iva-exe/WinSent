<script>
	import '../app.css';
	import '@fontsource/space-grotesk/400.css';
	import '@fontsource/space-grotesk/500.css';
	import '@fontsource/space-grotesk/600.css';
	import '@fontsource/fira-mono/400.css';
	import '@fontsource/fira-mono/500.css';

	import { onMount } from 'svelte';
	import { fade } from 'svelte/transition';
	import { page } from '$app/state';
	import { invoke } from '@tauri-apps/api/core';
	import { getCurrentWindow } from '@tauri-apps/api/window';
	import { daemon, startDaemonPolling } from '$lib/daemon.svelte.js';
	import ItemMenu from '$lib/ItemMenu.svelte';
	import { updater, startUpdateChecks, runUpdate } from '$lib/updater.svelte.js';
	import {
		House,
		Activity,
		Blocks,
		Files,
		Search,
		ListStart,
		Users,
		Cpu,
		BrainCircuit,
		Wifi,
		Router,
		Shield,
		Settings,
		History,
		Download,
		Minus,
		Square,
		TriangleAlert,
		X
	} from 'lucide-svelte';

	let { children } = $props();

	// Navigace dle Frame 5 — pořadí i názvy sekcí jsou závazné.
	const nav = [
		{ href: '/home', label: 'Home', icon: House },
		{ href: '/tasks', label: 'Tasks', icon: Activity },
		{ href: '/incidents', label: 'Incidents', icon: TriangleAlert },
		{ href: '/programs', label: 'Programs', icon: Blocks },
		{ href: '/files', label: 'Files', icon: Files },
		{ href: '/search', label: 'Vyhledávání', icon: Search },
		{ href: '/onstart', label: 'On start', icon: ListStart },
		{ href: '/users', label: 'Users', icon: Users },
		{ href: '/hardware', label: 'Hardware', icon: Cpu },
		{ href: '/drivers', label: 'Drivers', icon: BrainCircuit },
		{ href: '/connection', label: 'Connection', icon: Wifi },
		{ href: '/network', label: 'Network', icon: Router },
		{ href: '/security', label: 'Security', icon: Shield }
	];

	// Lazy — mimo Tauri (preview v prohlížeči) getCurrentWindow neexistuje.
	function win() {
		try {
			return getCurrentWindow();
		} catch {
			return null;
		}
	}

	// Uptime systému (z démona, GetTickCount64) — poll 1×/5 s.
	let sysUptime = $state(null);

	// Zvednutí zastavené služby. Instalátor si o práva správce řekne sám,
	// UI jen počká — jakmile služba naběhne, ukazatel se rozsvítí pollem.
	let fixing = $state(false);
	let fixHint = $state('Spustit službu (vyžádá si práva správce)');
	async function fixService() {
		fixing = true;
		try {
			await invoke('repair_service');
			fixHint = 'Instalátor běží — potvrď výzvu Windows';
		} catch (e) {
			fixHint = String(e);
		}
		// Instalátor chvíli stahuje a kontroluje; dřív než za pár sekund
		// nemá smysl tlačítko vracet.
		setTimeout(() => (fixing = false), 8000);
	}

	function fmtUp(s) {
		if (s == null) return '—';
		const d = Math.floor(s / 86400);
		const h = Math.floor((s % 86400) / 3600);
		const m = Math.floor((s % 3600) / 60);
		return d > 0 ? `${d} d ${h} h` : h > 0 ? `${h} h ${m} m` : `${m} m`;
	}

	// Badge zdraví na navigaci (SPEC 9.2): incidenty za 24 h → Incidents,
	// plný disk / opotřebený SSD → Files. Jen upozornění, ne poplach.
	let badges = $state({});
	async function pollBadges() {
		const b = {};
		try {
			const inc = await invoke('query_incidents', { limit: 50 });
			const day = Date.now() / 1000 - 86400;
			const recent = inc.filter((i) => i.ts > day);
			if (recent.length) {
				b['/incidents'] = {
					count: recent.length,
					color: recent.some((i) => i.kind === 'bsod') ? 'var(--danger)' : 'var(--warn)'
				};
			}
		} catch {
			/* služba mimo */
		}
		try {
			const v = await invoke('query_volumes');
			const full = v.volumes.some(
				(x) => x.total_bytes && (x.total_bytes - x.free_bytes) / x.total_bytes >= 0.9
			);
			const worn = v.health.some((h) => (h.used_pct ?? 0) >= 80 || h.critical);
			if (full || worn) {
				b['/files'] = { count: null, color: worn ? 'var(--danger)' : 'var(--warn)' };
			}
		} catch {
			/* služba mimo */
		}
		badges = b;
	}


	onMount(() => {
		startDaemonPolling();
		async function pollUptime() {
			try {
				const s = await invoke('query_system');
				sysUptime = s.uptime_s;
			} catch {
				sysUptime = null;
			}
		}
		pollUptime();
		pollBadges();
		startUpdateChecks();
		const t = setInterval(pollUptime, 5000);
		const t2 = setInterval(pollBadges, 60000);
		return () => {
			clearInterval(t);
			clearInterval(t2);
		};
	});
</script>

<!-- Spotlight lišta je samostatné okno, ne sekce v aplikaci: nemá kolem
     sebe nic z jejího rámu — žádnou navigaci, titulek ani stavový řádek.
     Rozhoduje se to tady, ne přes reset layoutu SvelteKitu, protože
     styly aplikace (app.css) potřebuje i lišta. -->
{#if page.url.pathname.startsWith('/spotlight')}
	{@render children()}
	<ItemMenu />
{:else}
<div class="app">
	<!-- ── Titlebar (vlastní, drag region) ─────────────────────── -->
	<header class="titlebar" data-tauri-drag-region>
		<div class="brand" data-tauri-drag-region>
			<img src="/icon.png" alt="" width="20" height="20" draggable="false" />
			<span class="wordmark">Winsent</span>
		</div>

		<div class="daemon" title={daemon.detail} data-tauri-drag-region>
			<span class="dot" class:alive={daemon.alive}></span>
			<span class="daemon-label">{daemon.alive ? 'služba běží' : 'služba neběží'}</span>
			<!-- Zastavená služba není jen zpráva, ale i cesta ven: klik
			     pustí instalátor v opravném režimu (vyžádá si práva
			     správce). Rozhodnutí zůstává na uživateli. -->
			{#if !daemon.alive}
				<button class="fix" onclick={fixService} disabled={fixing} title={fixHint}>
					{fixing ? 'spouštím…' : 'spustit'}
				</button>
			{/if}
		</div>

		<!-- Uptime systému a démona -->
		<div class="uptimes label-tech" data-tauri-drag-region>
			<span title="Uptime systému">sys {fmtUp(sysUptime)}</span>
			<span class="sep">·</span>
			<span title="Uptime démona">daemon {daemon.alive ? fmtUp(daemon.uptime_s) : '—'}</span>
		</div>

		<div class="win-controls">
			<button class="wc" title="Minimalizovat" onclick={() => win()?.minimize()}>
				<Minus size={17} strokeWidth={1.75} />
			</button>
			<button class="wc" title="Maximalizovat" onclick={() => win()?.toggleMaximize()}>
				<Square size={14} strokeWidth={1.75} />
			</button>
			<button class="wc close" title="Zavřít" onclick={() => win()?.close()}>
				<X size={18} strokeWidth={1.75} />
			</button>
		</div>
	</header>

	<!-- ── Tělo: sidebar + obsahový panel ──────────────────────── -->
	<div class="body">
		<nav class="sidebar">
			<ul>
				{#each nav as item (item.href)}
					<li>
						<a href={item.href} class:active={page.url.pathname.startsWith(item.href)}>
							<item.icon size={21} strokeWidth={1.75} />
							<span>{item.label}</span>
							{#if badges[item.href]}
								<span
									class="nav-badge"
									style:background={badges[item.href].color}
									title="vyžaduje pozornost"
									>{badges[item.href].count ?? ''}</span
								>
							{/if}
						</a>
					</li>
				{/each}
			</ul>
			<div class="sidebar-bottom">
				<a href="/settings" class:active={page.url.pathname.startsWith('/settings')}>
					<Settings size={21} strokeWidth={1.75} />
					<span>Settings</span>
				</a>
				<!-- Historie zásahů do systému (audit) — vedle nastavení. -->
				<a
					href="/history"
					class="hist"
					class:active={page.url.pathname.startsWith('/history')}
					title="Historie zásahů do systému"
				>
					<History size={21} strokeWidth={1.75} />
					<span>Historie</span>
				</a>
			</div>
		</nav>

		<main class="content">
			<!-- Jemný fade při přepnutí sekce — žádné vjezdy (DESIGN.md kap. 9). -->
			{#key page.url.pathname}
				<div class="route" in:fade={{ duration: 150 }}>
					{@render children()}
				</div>
			{/key}
		</main>
	</div>

	<!-- Kontextové menu položek. Jedno pro celou aplikaci — sekce mu jen
	     řeknou, na co se kliklo (viz lib/itemmenu.svelte.js). -->
	<ItemMenu />

	<!-- ── Nová verze: trvalé upozornění vpravo dole ──
	     Nemizí samo a nedá se odkliknout: stará verze je stav, který
	     platí, dokud se neaktualizuje. Zavírací křížek by z toho udělal
	     oznámení, které si člověk odbaví a zapomene. -->
	{#if updater.available}
		<div class="upd" transition:fade={{ duration: 200 }}>
			<Download size={17} />
			<div class="upd-text">
				<b>Je dostupná nová verze</b>
				<span class="upd-ver">
					máš <span class="mono">{updater.current}</span> · nová
					<span class="mono">{updater.latest}</span>
				</span>
				{#if updater.runError}
					<span class="upd-err">{updater.runError}</span>
				{/if}
			</div>
			<button class="upd-btn" disabled={updater.busy} onclick={runUpdate}>
				{updater.busy ? 'stahuji…' : 'Aktualizovat'}
			</button>
		</div>
	{/if}
</div>
{/if}

<style>
	/* Upozornění na novou verzi — vpravo dole, nad obsahem, trvale.
	   Jantarová, ne červená: není to porucha, jen je co stáhnout. */
	.upd {
		position: fixed;
		right: 16px;
		bottom: 16px;
		z-index: 40;
		display: flex;
		align-items: center;
		gap: 12px;
		max-width: 420px;
		padding: 12px 14px;
		border: 1px solid color-mix(in srgb, var(--warn) 45%, transparent);
		border-radius: var(--radius-lg);
		background: color-mix(in srgb, var(--warn) 12%, var(--panel));
		backdrop-filter: blur(12px);
		box-shadow: 0 8px 28px rgba(0, 0, 0, 0.45);
		color: var(--text);
	}
	.upd > :global(svg) {
		flex: none;
		color: var(--warn);
	}
	.upd-text {
		display: flex;
		flex-direction: column;
		gap: 2px;
		min-width: 0;
		font-size: var(--fs-sm);
	}
	.upd-ver {
		color: var(--text-dim);
		font-size: var(--fs-xs);
	}
	.upd-ver .mono {
		font-family: var(--font-mono);
	}
	.upd-err {
		color: var(--danger);
		font-size: var(--fs-xs);
	}
	.upd-btn {
		flex: none;
		padding: 7px 14px;
		border: none;
		border-radius: var(--radius-sm);
		background: var(--warn);
		color: #1b1200;
		font: inherit;
		font-size: var(--fs-sm);
		font-weight: 600;
		cursor: pointer;
	}
	.upd-btn:hover:not(:disabled) {
		filter: brightness(1.12);
	}
	.upd-btn:disabled {
		opacity: 0.6;
		cursor: wait;
	}
	.app {
		display: flex;
		flex-direction: column;
		height: 100vh;
		/* Poloprůhledné pozadí nechává prosvítat blur okna (jediný
		   povolený gradient — zadní plocha, DESIGN.md kap. 2). Rohy
		   a rám okna nechává systém — žádný vlastní border/radius. */
		background: radial-gradient(
			120% 90% at 20% 0%,
			rgba(20, 21, 26, 0.85) 0%,
			rgba(14, 15, 18, 0.82) 55%,
			rgba(11, 12, 15, 0.87) 100%
		);
	}

	/* ── Titlebar ── */
	.titlebar {
		display: flex;
		align-items: center;
		gap: 1.5rem;
		height: 44px;
		padding: 0 0.4rem 0 1rem;
		flex-shrink: 0;
	}
	.brand {
		display: flex;
		align-items: center;
		gap: 0.55rem;
		color: var(--accent);
	}
	.wordmark {
		font-weight: 600;
		font-size: 0.98rem;
		letter-spacing: 0.01em;
	}
	.daemon {
		display: flex;
		align-items: center;
		gap: 0.45rem;
	}
	.dot {
		width: 7px;
		height: 7px;
		border-radius: 50%;
		background: var(--danger);
		box-shadow: var(--glow-danger);
	}
	.dot.alive {
		background: var(--ok);
		box-shadow: var(--glow-ok);
	}
	.daemon-label {
		font-family: var(--font-mono);
		font-size: var(--fs-2xs);
		letter-spacing: 0.04em;
		color: var(--text-dim);
	}
	.fix {
		font-family: var(--font-mono);
		font-size: var(--fs-3xs);
		letter-spacing: 0.04em;
		padding: 0.1rem 0.4rem;
		border-radius: 4px;
		border: 1px solid var(--danger);
		background: transparent;
		color: var(--danger);
		cursor: pointer;
	}
	.fix:hover:not(:disabled) {
		background: var(--danger);
		color: var(--bg);
	}
	.fix:disabled {
		opacity: 0.55;
		cursor: default;
	}
	.uptimes {
		display: flex;
		gap: 0.5rem;
	}
	.uptimes .sep {
		color: var(--text-faint);
	}
	.win-controls {
		margin-left: auto;
		display: flex;
		align-items: center;
	}
	.wc {
		display: grid;
		place-items: center;
		width: 42px;
		height: 34px;
		border: 0;
		border-radius: var(--radius);
		background: transparent;
		color: var(--text-dim);
		cursor: default;
		transition: background 130ms ease-out, color 130ms ease-out;
	}
	.wc:hover {
		background: var(--surface-hover);
		color: var(--text);
	}
	.wc.close {
		color: var(--danger);
	}
	.wc.close:hover {
		background: color-mix(in srgb, var(--danger) 18%, transparent);
		color: var(--danger);
	}

	/* ── Tělo ── */
	.body {
		flex: 1;
		display: flex;
		gap: 10px;
		padding: 0 10px 10px;
		min-height: 0;
	}

	/* ── Sidebar (samostatný panel, Frame 5) ── */
	.sidebar {
		display: flex;
		flex-direction: column;
		width: 218px;
		flex-shrink: 0;
		padding: 0.6rem;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
	}
	.sidebar ul {
		list-style: none;
		margin: 0;
		padding: 0;
		display: flex;
		flex-direction: column;
		gap: 2px;
		overflow-y: auto;
	}
	.sidebar a {
		display: flex;
		align-items: center;
		gap: 0.8rem;
		padding: 0.62rem 0.75rem;
		border-radius: var(--radius);
		color: var(--text-dim);
		text-decoration: none;
		font-weight: 500;
		font-size: var(--fs-xl);
		transition: background 130ms ease-out, color 130ms ease-out;
	}
	.sidebar a:hover {
		background: var(--surface);
		color: var(--text);
	}
	.sidebar a.active {
		background: var(--surface-hover);
		color: var(--accent);
	}
	.sidebar-bottom {
		margin-top: auto;
		padding-top: 0.5rem;
		border-top: 1px dotted var(--border-strong);
	}

	/* ── Obsahový panel ── */
	.content {
		flex: 1;
		min-width: 0;
		background: var(--panel);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		overflow-y: auto;
		padding: 1.25rem 1.4rem;
	}
	.route {
		height: 100%;
		min-height: 0;
	}

	/* Badge zdraví na navigaci (SPEC 9.2). */
	.nav-badge {
		margin-left: auto;
		min-width: 8px;
		height: 8px;
		border-radius: 999px;
		font-family: var(--font-mono);
		font-size: 0.58rem;
		line-height: 1;
		color: #0e0f12;
		padding: 0;
		display: grid;
		place-items: center;
		box-shadow: 0 0 6px color-mix(in srgb, currentColor 50%, transparent);
	}
	.nav-badge:not(:empty) {
		min-width: 15px;
		height: 15px;
		padding: 0 3px;
	}
</style>
