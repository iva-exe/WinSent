<script>
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';

	// Stav démona pro indikátor v hlavičce (SPEC kap. 9.2):
	// zeleně = služba běží, červeně = neběží. Zdroj pravdy je vždy
	// odpověď přes pipe, nikdy ne domněnka UI.
	let daemon = $state({ alive: false, uptime_s: 0, detail: 'zjišťuji…' });

	async function refresh() {
		try {
			const pong = await invoke('ping_daemon');
			daemon = {
				alive: true,
				uptime_s: pong.uptime_s,
				detail: `uptime ${formatUptime(pong.uptime_s)} · protokol v${pong.protocol_version}`
			};
		} catch (e) {
			daemon = { alive: false, uptime_s: 0, detail: String(e) };
		}
	}

	function formatUptime(s) {
		const h = Math.floor(s / 3600);
		const m = Math.floor((s % 3600) / 60);
		return h > 0 ? `${h} h ${m} min` : m > 0 ? `${m} min ${s % 60} s` : `${s} s`;
	}

	onMount(() => {
		refresh();
		const timer = setInterval(refresh, 1500);
		return () => clearInterval(timer);
	});
</script>

<div class="shell">
	<!-- Hlavička: glassmorphism, trvalý indikátor stavu démona -->
	<header>
		<h1>syswatch</h1>
		<div class="daemon" title={daemon.detail}>
			<span class="dot" class:alive={daemon.alive}></span>
			<span class="label">{daemon.alive ? 'služba běží' : 'služba neběží'}</span>
		</div>
	</header>

	<main>
		<div class="card">
			<p class="placeholder">v0 — prázdný skelet</p>
			<p class="detail">{daemon.detail}</p>
			<p class="hint">Obsah přijde s v1 (živé procesy).</p>
		</div>
	</main>
</div>

<style>
	.shell {
		display: flex;
		flex-direction: column;
		height: 100vh;
	}

	/* ── Hlavička — glassmorphism, jemný border (SPEC 9.4) ── */
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.65rem 1.25rem;
		border-bottom: 1px solid var(--border);
		background: var(--surface);
		backdrop-filter: blur(18px);
	}
	h1 {
		margin: 0;
		font-family: var(--font-heading);
		font-size: 1.1rem;
		font-weight: 600;
		letter-spacing: 0.01em;
		color: var(--accent);
	}

	/* ── Indikátor démona — barva výhradně podle stavu ── */
	.daemon {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		padding: 0.25rem 0.7rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--surface);
		font-size: 0.82rem;
		color: var(--text-dim);
	}
	.dot {
		width: 8px;
		height: 8px;
		border-radius: 50%;
		background: var(--danger);
		box-shadow: 0 0 6px color-mix(in srgb, var(--danger) 60%, transparent);
	}
	.dot.alive {
		background: var(--ok);
		box-shadow: 0 0 6px color-mix(in srgb, var(--ok) 60%, transparent);
	}

	/* ── Obsah ── */
	main {
		flex: 1;
		display: grid;
		place-items: center;
	}
	.card {
		display: flex;
		flex-direction: column;
		align-items: center;
		gap: 0.35rem;
		padding: 1.5rem 2.5rem;
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		background: var(--surface);
	}
	.placeholder {
		margin: 0;
		color: var(--text);
		font-size: 0.95rem;
	}
	.detail {
		margin: 0;
		color: var(--text-dim);
		font-size: 0.82rem;
	}
	.hint {
		margin: 0;
		color: var(--text-faint);
		font-size: 0.78rem;
	}
</style>
