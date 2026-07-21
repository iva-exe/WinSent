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
	import {
		House,
		Activity,
		Blocks,
		Files,
		ListStart,
		Users,
		Cpu,
		BrainCircuit,
		Wifi,
		Router,
		Shield,
		Settings,
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

	function fmtUp(s) {
		if (s == null) return '—';
		const d = Math.floor(s / 86400);
		const h = Math.floor((s % 86400) / 3600);
		const m = Math.floor((s % 3600) / 60);
		return d > 0 ? `${d} d ${h} h` : h > 0 ? `${h} h ${m} m` : `${m} m`;
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
		const t = setInterval(pollUptime, 5000);
		return () => clearInterval(t);
	});
</script>

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
		</div>

		<!-- Uptime systému a démona -->
		<div class="uptimes label-tech" data-tauri-drag-region>
			<span title="Uptime systému">sys {fmtUp(sysUptime)}</span>
			<span class="sep">·</span>
			<span title="Uptime démona">daemon {daemon.alive ? fmtUp(daemon.uptime_s) : '—'}</span>
		</div>

		<div class="win-controls">
			<button class="wc" title="Minimalizovat" onclick={() => win()?.minimize()}>
				<Minus size={16} strokeWidth={1.75} />
			</button>
			<button class="wc" title="Maximalizovat" onclick={() => win()?.toggleMaximize()}>
				<Square size={13} strokeWidth={1.75} />
			</button>
			<button class="wc close" title="Zavřít" onclick={() => win()?.close()}>
				<X size={17} strokeWidth={1.75} />
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
							<item.icon size={20} strokeWidth={1.75} />
							<span>{item.label}</span>
						</a>
					</li>
				{/each}
			</ul>
			<div class="sidebar-bottom">
				<a href="/settings" class:active={page.url.pathname.startsWith('/settings')}>
					<Settings size={20} strokeWidth={1.75} />
					<span>Settings</span>
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
</div>

<style>
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
		font-size: 11px;
		letter-spacing: 0.04em;
		color: var(--text-dim);
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
		font-size: 0.92rem;
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
</style>
