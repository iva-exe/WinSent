// Sdílený stav démona pro celý shell (indikátor v titlebaru i obsah).
// Zdroj pravdy je vždy odpověď přes pipe, nikdy domněnka UI.
import { invoke } from '@tauri-apps/api/core';

export const daemon = $state({
	alive: false,
	uptime_s: 0,
	detail: 'zjišťuji…'
});

async function refresh() {
	try {
		const pong = await invoke('ping_daemon');
		daemon.alive = true;
		daemon.uptime_s = pong.uptime_s;
		daemon.detail = `uptime ${formatUptime(pong.uptime_s)} · protokol v${pong.protocol_version}`;
	} catch (e) {
		daemon.alive = false;
		daemon.uptime_s = 0;
		daemon.detail = String(e);
	}
}

export function formatUptime(s) {
	const h = Math.floor(s / 3600);
	const m = Math.floor((s % 3600) / 60);
	return h > 0 ? `${h} h ${m} min` : m > 0 ? `${m} min ${s % 60} s` : `${s} s`;
}

let timer;

// Spustí polling (idempotentní — druhé volání nic nezmění).
export function startDaemonPolling() {
	if (timer) return;
	refresh();
	timer = setInterval(refresh, 1500);
}
