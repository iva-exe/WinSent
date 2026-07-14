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
				detail: `služba běží (uptime ${formatUptime(pong.uptime_s)})`
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
	<header>
		<h1>syswatch</h1>
		<div class="daemon" title={daemon.detail}>
			<span class="dot" class:alive={daemon.alive}></span>
			<span class="label">{daemon.alive ? 'služba běží' : 'služba neběží'}</span>
		</div>
	</header>

	<main>
		<p class="placeholder">
			v0 — prázdný skelet. Obsah přijde s v1 (živé procesy).
		</p>
		<p class="detail">{daemon.detail}</p>
	</main>
</div>

<style>
	/* Tech + minimalismus, tmavý základ (SPEC kap. 9.4). */
	:global(body) {
		background: #101014;
		color: #e8e8ea;
		font-family: 'Inter', 'Segoe UI', system-ui, sans-serif;
	}
	.shell {
		display: flex;
		flex-direction: column;
		height: 100vh;
	}
	header {
		display: flex;
		align-items: center;
		justify-content: space-between;
		padding: 0.75rem 1.25rem;
		border-bottom: 1px solid rgba(255, 255, 255, 0.08);
		background: rgba(255, 255, 255, 0.03);
	}
	h1 {
		margin: 0;
		font-size: 1.05rem;
		font-weight: 600;
		letter-spacing: 0.02em;
	}
	.daemon {
		display: flex;
		align-items: center;
		gap: 0.5rem;
		font-size: 0.85rem;
		color: #b8b8bd;
	}
	.dot {
		width: 10px;
		height: 10px;
		border-radius: 50%;
		background: #e5484d; /* neběží = červená */
	}
	.dot.alive {
		background: #46a758; /* běží = zelená */
	}
	main {
		flex: 1;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 0.5rem;
	}
	.placeholder {
		color: #8a8a90;
		font-size: 0.95rem;
	}
	.detail {
		color: #55555c;
		font-size: 0.8rem;
	}
</style>
