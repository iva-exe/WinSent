<script>
	import { invoke } from '@tauri-apps/api/core';
	import { onMount } from 'svelte';
	import { untrack } from 'svelte';
	import { daemon } from '$lib/daemon.svelte.js';
	import LiveChart from '$lib/LiveChart.svelte';
	import Sparkline from '$lib/Sparkline.svelte';
	import Num from '$lib/Num.svelte';
	import {
		Cpu,
		MemoryStick,
		Zap,
		ArrowDown,
		ArrowUp,
		HardDrive,
		Lock,
		ChevronRight
	} from 'lucide-svelte';

	// Buffery na celou dostupnou historii (1 h retence surových vzorků).
	const CAP = 3600;
	// Tabulka: hodnoty se mění každou sekundu, ale PŘESKUPUJE se nejvýš
	// jednou za REORDER_MS — jinak list neustále tancuje (výkon i klid).
	const REORDER_MS = 3000;

	let ts = $state([]);
	let cpu = $state([]);
	let mem = $state([]);
	let sys = $state([]);
	let gpu = $state([]);
	let down = $state([]);
	let up = $state([]);

	let system = $state(null);
	let procs = [];
	let error = $state('');
	let historyLoaded = false;

	// Metrika grafu — procentní režimy mají gradient, Síť/Disk dvě série.
	let mode = $state('sys');
	const modes = [
		{ id: 'sys', label: 'System' },
		{ id: 'cpu', label: 'CPU' },
		{ id: 'ram', label: 'RAM' },
		{ id: 'gpu', label: 'GPU' },
		{ id: 'disk', label: 'Disk' },
		{ id: 'net', label: 'Síť' }
	];
	const chartValues = $derived(
		mode === 'cpu' ? cpu : mode === 'ram' ? mem : mode === 'gpu' ? gpu : mode === 'net' ? down : sys
	);
	const chartValues2 = $derived(mode === 'net' ? up : null);

	// Statické info komponent (názvy, RAM moduly…) — jednou ze služby.
	let statics = $state(null);
	async function loadStatics() {
		try {
			statics = await invoke('query_sys_info');
		} catch {
			setTimeout(loadStatics, 3000);
		}
	}

	// Per-disk série pro záložku Disk: index → {ts, r, w}.
	let diskSeries = $state({});
	// Součet disků (pro readout a dlaždici System).
	let diskTot = $state([]);

	// Zátěž systému: průměr komponent tažený k maximu podle výše maxima
	// (CPU 100 + RAM 20 ⇒ 100; CPU 20 + RAM 60 ⇒ ~52). GPU se počítá,
	// jen když je dostupné.
	function sysLoad(components) {
		const vals = components.filter((v) => v != null && !Number.isNaN(v));
		if (!vals.length) return 0;
		const mean = vals.reduce((a, b) => a + b, 0) / vals.length;
		const max = Math.max(...vals);
		const w = Math.min(max / 100, 1);
		return mean * (1 - w) + max * w;
	}

	// Hover a zámek času (klik do grafu). Zámek má přednost.
	let hover = $state(null);
	let pinned = $state(null);
	const focusTs = $derived(pinned ?? hover?.t ?? null);
	const focusIdx = $derived(focusTs == null ? null : nearestIdx(focusTs));

	function nearestIdx(t) {
		if (!ts.length) return null;
		let lo = 0;
		let hi = ts.length - 1;
		while (lo < hi) {
			const mid = (lo + hi) >> 1;
			if (ts[mid] < t) lo = mid + 1;
			else hi = mid;
		}
		if (lo > 0 && Math.abs(ts[lo - 1] - t) < Math.abs(ts[lo] - t)) lo -= 1;
		return lo;
	}

	// Stav tasků a detailů z minulosti — VÝHRADNĚ pro zámek (klik do
	// grafu). Hover ukazuje jen hodnoty v hlavičce, list nechává živý.
	let histProcs = $state(null);
	let histDetail = $state(null);
	// Okno historie jader kolem zámku (±30 s) pro mini grafy s linkou.
	let pinnedCores = $state(null); // { byCore: number[][], marker: number }
	let histTimer = null;

	// Závislost POUZE na `pinned` (untrack) — jinak by efekt reagoval
	// i na každý tick dat a stahoval historii pořád dokola.
	$effect(() => {
		const t = pinned;
		untrack(() => {
			clearTimeout(histTimer);
			if (t == null) {
				if (histProcs || histDetail || pinnedCores) {
					histProcs = null;
					histDetail = null;
					pinnedCores = null;
					refreshTable(true);
				}
				return;
			}
			histTimer = setTimeout(async () => {
				const ts0 = Math.round(t);
				try {
					histProcs = await invoke('query_procs_at', { ts: ts0 });
				} catch {
					histProcs = null;
				}
				try {
					histDetail = await invoke('query_detail_at', { ts: ts0 });
				} catch {
					histDetail = null;
				}
				// Okno jader ±30 s — zamčený bod vyjde doprostřed mini grafů.
				try {
					const pts = await invoke('query_core_history', { from: ts0 - 30, to: ts0 + 30 });
					const tsSet = [...new Set(pts.map((p) => p[0]))].sort((a, b) => a - b);
					const tsIdx = new Map(tsSet.map((v, i) => [v, i]));
					const byCore = [];
					for (const [pt, core, pct] of pts) {
						(byCore[core] ??= new Array(tsSet.length).fill(null))[tsIdx.get(pt)] = pct;
					}
					const target = histDetail?.ts ?? ts0;
					let marker = tsSet.findIndex((v) => v >= target);
					if (marker === -1) marker = tsSet.length - 1;
					pinnedCores = { byCore, marker };
				} catch {
					pinnedCores = null;
				}
				refreshTable(true);
				// Ikony i pro identity_key z náhledu historie.
				refreshIcons();
			}, 150);
		});
	});

	// Zdroje detail sekce: při zámku data z historie, jinak živá.
	const dCores = $derived(pinned != null && histDetail ? histDetail.cores : (system?.cores ?? []));
	const dDisks = $derived(pinned != null && histDetail ? histDetail.disks : (system?.disks ?? []));
	const dGpu = $derived(
		pinned != null && histDetail
			? histDetail.gpu && {
					...histDetail.gpu,
					vram_total_mb: system?.gpu?.vram_total_mb ?? null
				}
			: system?.gpu
	);
	// Hodnoty dlaždic System/RAM/Síť při zámku z hlavních bufferů.
	const dIdx = $derived(pinned != null ? focusIdx : null);
	const dCpuPct = $derived(dIdx != null ? cpu[dIdx] : system?.cpu_pct);
	const dMemPct = $derived(dIdx != null ? mem[dIdx] : mem.at(-1));
	const dGpuPct = $derived(dIdx != null ? gpu[dIdx] : system?.gpu_pct);
	const dDown = $derived(dIdx != null ? down[dIdx] : system?.net_rx_bps);
	const dUp = $derived(dIdx != null ? up[dIdx] : system?.net_tx_bps);
	const dDiskTot = $derived(dIdx != null ? diskTot[dIdx] : diskTot.at(-1));

	// Název komponenty do hlavičky detailu (SPEC 15.4 — vše o komponentě
	// pohromadě u ní).
	const ramSummary = $derived.by(() => {
		const mods = statics?.ram_modules ?? [];
		if (!mods.length) return '';
		const totalGb = mods.reduce((a, m) => a + m.size_mb, 0) / 1024;
		const speed = mods[0]?.configured_mts || mods[0]?.speed_mts || 0;
		return `${totalGb.toFixed(0)} GB (${mods.length}×) @ ${speed} MT/s`;
	});
	const detailName = $derived(
		mode === 'cpu'
			? (statics?.cpu_name ?? '')
			: mode === 'gpu'
				? (statics?.gpu_name ?? '')
				: mode === 'ram'
					? ramSummary
					: mode === 'disk'
						? `${statics?.disks?.length ?? 0} fyzických`
						: ''
	);

	// ── Detail sekce: historie jader (sparklines) + síťové špičky ──
	let coresHist = $state([]);
	let peakDown = $state(0);
	let peakUp = $state(0);

	// Barva podle zátěže (0–100) — stejné zastávky jako gradient grafu.
	const OK_C = [74, 222, 128];
	const WARN_C = [245, 158, 11];
	const DANGER_C = [239, 68, 68];
	const TEMP_COLD = [124, 192, 255];

	const lerpC = (c1, c2, t) => c1.map((v, i) => Math.round(v + (c2[i] - v) * t));
	const rgb = (c) => `rgb(${c[0]}, ${c[1]}, ${c[2]})`;

	function colorForLoad(v) {
		if (v == null) return 'var(--text-dim)';
		if (v <= 55) return rgb(OK_C);
		if (v <= 75) return rgb(lerpC(OK_C, WARN_C, (v - 55) / 20));
		if (v <= 90) return rgb(lerpC(WARN_C, DANGER_C, (v - 75) / 15));
		return rgb(DANGER_C);
	}

	// Teplota: světle modrá (chladno) → oranžová (střed) → červená (moc).
	function colorForTemp(t) {
		if (t == null) return 'var(--text-dim)';
		if (t <= 35) return rgb(TEMP_COLD);
		if (t <= 60) return rgb(lerpC(TEMP_COLD, WARN_C, (t - 35) / 25));
		if (t <= 80) return rgb(lerpC(WARN_C, DANGER_C, (t - 60) / 20));
		return rgb(DANGER_C);
	}

	// ── Tabulka: zmrazené pořadí mezi reordery ──
	let displayRows = $state([]);
	let sortKey = $state('sys_pct');
	let sortDir = $state(-1);
	// Filtr (v1 DoD: řazení + filtr) — jméno nebo PID.
	let filter = $state('');
	// Pohled: seskupené aplikace (default, v2) / plochý seznam procesů.
	let viewMode = $state('apps');
	let lastOrderAt = 0;

	const visibleRows = $derived.by(() => {
		const q = filter.trim().toLowerCase();
		if (!q) return displayRows;
		return displayRows.filter(
			(r) => r.name.toLowerCase().includes(q) || String(r.pid).includes(q)
		);
	});

	function buildRows() {
		const total = (system?.mem_total_mb ?? 0) * 1024 * 1024;
		const src = histProcs ? histProcs.rows : procs;
		return src.map((p) => ({
			pid: p.pid,
			name: p.name,
			cpu_pct: p.cpu_pct,
			ws_bytes: p.ws_bytes,
			threads: p.threads ?? null,
			disk_bps: (p.disk_r_bps ?? 0) + (p.disk_w_bps ?? 0),
			gpu_pct: p.gpu_pct ?? 0,
			sys_pct: sysLoad([p.cpu_pct, total > 0 ? (p.ws_bytes / total) * 100 : null]),
			// Identita aplikace (v2). Historie ji nemá → fallback na jméno.
			identity_key: p.identity_key ?? `name:${p.name}`,
			app_name: p.app_name ?? p.name,
			// '' místo null → řazení podle vydavatele funguje stringově.
			publisher: p.publisher ?? '',
			protection: p.protection ?? 'user',
			confidence: p.confidence ?? 'exact'
		}));
	}

	// Rozbalené skupiny (strom aplikace → procesy).
	let expanded = $state(new Set());
	function toggleGroup(key) {
		const s = new Set(expanded);
		if (s.has(key)) s.delete(key);
		else s.add(key);
		expanded = s;
	}

	// Skupiny odvozené z displayRows (pořadí i přeskupení už throttlované).
	// Pořadí skupin = první výskyt člena → stabilní stejně jako list.
	const groups = $derived.by(() => {
		const map = new Map();
		for (const p of visibleRows) {
			let g = map.get(p.identity_key);
			if (!g) {
				g = {
					key: p.identity_key,
					app_name: p.app_name,
					publisher: p.publisher,
					// Nejpřísnější třída ve skupině (pro ikonu zámku).
					protection: p.protection,
					confidence: p.confidence,
					children: [],
					cpu_pct: 0,
					ws_bytes: 0,
					disk_bps: 0,
					gpu_pct: 0,
					sys_pct: 0
				};
				map.set(p.identity_key, g);
			}
			g.children.push(p);
			g.cpu_pct += p.cpu_pct;
			g.ws_bytes += p.ws_bytes;
			g.disk_bps += p.disk_bps;
			g.gpu_pct += p.gpu_pct;
			if (protRank(p.protection) < protRank(g.protection)) g.protection = p.protection;
			if (p.confidence === 'guess') g.confidence = 'guess';
		}
		// Sys zátěž skupiny ze součtu CPU + podílu RAM.
		const total = (system?.mem_total_mb ?? 0) * 1024 * 1024;
		for (const g of map.values()) {
			g.sys_pct = sysLoad([g.cpu_pct, total > 0 ? (g.ws_bytes / total) * 100 : null]);
		}
		return [...map.values()];
	});

	// Pořadí přísnosti pro výběr ikony skupiny.
	function protRank(p) {
		return { critical: 0, protected: 1, system: 2, user: 3 }[p] ?? 3;
	}

	function sortRows(rows) {
		return rows.sort((a, b) => {
			const va = a[sortKey];
			const vb = b[sortKey];
			const cmp = typeof va === 'string' ? va.localeCompare(vb) : (va ?? -1) - (vb ?? -1);
			// Stabilní dorovnání PIDem — stejné hodnoty se nepřehazují.
			return cmp !== 0 ? cmp * sortDir : a.pid - b.pid;
		});
	}

	function refreshTable(force = false) {
		const rows = buildRows();
		const now = Date.now();
		if (force || now - lastOrderAt >= REORDER_MS || displayRows.length === 0) {
			lastOrderAt = now;
			displayRows = sortRows(rows);
			return;
		}
		// Jen aktualizace hodnot: řádky drží pozice, nové na konec,
		// zaniklé zmizí. Přeskupí se až příští reorder.
		const map = new Map(rows.map((r) => [r.pid, r]));
		const kept = [];
		for (const r of displayRows) {
			const cur = map.get(r.pid);
			if (cur) {
				kept.push(cur);
				map.delete(r.pid);
			}
		}
		displayRows = [...kept, ...map.values()];
	}

	function setSort(key) {
		if (sortKey === key) {
			sortDir = -sortDir;
		} else {
			sortKey = key;
			sortDir = key === 'name' || key === 'publisher' ? 1 : -1;
		}
		refreshTable(true);
	}

	const push = (arr, v) => [...arr.slice(-(CAP - 1)), v];

	async function pollSystem() {
		try {
			const s = await invoke('query_system');
			system = s;
			error = '';
			const memPct = (s.mem_used_mb / Math.max(s.mem_total_mb, 1)) * 100;
			const sysPct = sysLoad([s.cpu_pct, memPct, s.gpu_pct]);
			const now = Math.floor(Date.now() / 1000);
			ts = push(ts, now);
			cpu = push(cpu, s.cpu_pct);
			mem = push(mem, memPct);
			sys = push(sys, sysPct);
			gpu = push(gpu, s.gpu_pct);
			down = push(down, s.net_rx_bps);
			up = push(up, s.net_tx_bps);

			// Detail sekce: krátká historie jader + síťové špičky session.
			coresHist = (s.cores ?? []).map((v, i) => [
				...(coresHist[i] ?? []).slice(-59),
				v
			]);
			if (s.net_rx_bps > peakDown) peakDown = s.net_rx_bps;
			if (s.net_tx_bps > peakUp) peakUp = s.net_tx_bps;

			// Disky: per-disk série + celkový součet.
			let tot = 0;
			const nextSeries = { ...diskSeries };
			for (const d of s.disks ?? []) {
				tot += d.r_bps + d.w_bps;
				const cur = nextSeries[d.index] ?? { ts: [], r: [], w: [] };
				nextSeries[d.index] = {
					ts: push(cur.ts, now),
					r: push(cur.r, d.r_bps),
					w: push(cur.w, d.w_bps)
				};
			}
			diskSeries = nextSeries;
			diskTot = push(diskTot, tot);

			if (!historyLoaded) {
				historyLoaded = true;
				loadHistory(s, now);
			}
		} catch (e) {
			system = null;
			error = String(e);
		}
	}

	async function loadHistory(s, now) {
		try {
			const points = await invoke('query_system_history', { from: now - CAP, to: now - 1 });
			if (!points.length) return;
			const total = Math.max(s.mem_total_mb, 1);
			const hTs = [], hCpu = [], hMem = [], hSys = [], hGpu = [], hDown = [], hUp = [];
			for (const p of points) {
				const memPct = (p.mem_used_mb / total) * 100;
				hTs.push(p.ts);
				hCpu.push(p.cpu_pct);
				hMem.push(memPct);
				hSys.push(sysLoad([p.cpu_pct, memPct, p.gpu_pct]));
				hGpu.push(p.gpu_pct);
				hDown.push(p.net_rx_bps ?? 0);
				hUp.push(p.net_tx_bps ?? 0);
			}
			const cut = ts.length && hTs.length ? hTs.findIndex((t) => t >= ts[0]) : -1;
			const end = cut === -1 ? hTs.length : cut;
			ts = [...hTs.slice(0, end), ...ts];
			cpu = [...hCpu.slice(0, end), ...cpu];
			mem = [...hMem.slice(0, end), ...mem];
			sys = [...hSys.slice(0, end), ...sys];
			gpu = [...hGpu.slice(0, end), ...gpu];
			down = [...hDown.slice(0, end), ...down];
			up = [...hUp.slice(0, end), ...up];
			// diskTot zarovnáme s hlavní osou: součty z disk historie per ts.
			const totByTs = new Map();
			try {
				const dpts = await invoke('query_disk_history', { from: now - CAP, to: now - 1 });
				const series = {};
				for (const [t, idx, r, w] of dpts) {
					totByTs.set(t, (totByTs.get(t) ?? 0) + r + w);
					const cur = series[idx] ?? { ts: [], r: [], w: [] };
					cur.ts.push(t);
					cur.r.push(r);
					cur.w.push(w);
					series[idx] = cur;
				}
				// Prepend před živé body per disk.
				const merged = { ...diskSeries };
				for (const [idx, h] of Object.entries(series)) {
					const live = merged[idx] ?? { ts: [], r: [], w: [] };
					const c = live.ts.length ? h.ts.findIndex((t) => t >= live.ts[0]) : -1;
					const e = c === -1 ? h.ts.length : c;
					merged[idx] = {
						ts: [...h.ts.slice(0, e), ...live.ts],
						r: [...h.r.slice(0, e), ...live.r],
						w: [...h.w.slice(0, e), ...live.w]
					};
				}
				diskSeries = merged;
			} catch {
				// disk historie není fatální
			}
			diskTot = [...hTs.slice(0, end).map((t) => totByTs.get(t) ?? 0), ...diskTot];
		} catch {
			// historie není fatální
		}
	}

	async function pollProcs() {
		try {
			procs = await invoke('query_procs');
		} catch {
			procs = [];
		}
		if (!histProcs) refreshTable();
		refreshIcons();
	}

	// ── Ikony aplikací (v2) ──
	// Cache identity_key → data URL. Ikonu extrahuje služba z .exe na
	// background vlákně; UI si ji vyžádá jednou a vykreslí na canvas.
	let iconUrls = $state({});
	const iconState = new Map(); // key → 'pending' | 'done' | počet neúspěchů

	function rgbaToUrl(icon) {
		const canvas = document.createElement('canvas');
		canvas.width = icon.w;
		canvas.height = icon.h;
		const ctx = canvas.getContext('2d');
		const img = ctx.createImageData(icon.w, icon.h);
		img.data.set(icon.rgba);
		ctx.putImageData(img, 0, 0);
		return canvas.toDataURL('image/png');
	}

	async function fetchIcon(key) {
		const st = iconState.get(key);
		if (st === 'pending' || st === 'done') return;
		if (typeof st === 'number' && st >= 6) return; // vzdáno po 6 pokusech
		iconState.set(key, 'pending');
		try {
			const icon = await invoke('query_icon', { identityKey: key });
			if (icon) {
				iconUrls = { ...iconUrls, [key]: rgbaToUrl(icon) };
				iconState.set(key, 'done');
			} else {
				// worker ji ještě nedodělal → povolit další pokus
				iconState.set(key, (typeof st === 'number' ? st : 0) + 1);
			}
		} catch {
			iconState.set(key, (typeof st === 'number' ? st : 0) + 1);
		}
	}

	function refreshIcons() {
		const keys = new Set();
		for (const p of procs) keys.add(p.identity_key ?? `name:${p.name}`);
		// I klíče z náhledu historie — ikony jsou v cache služby.
		for (const p of histProcs?.rows ?? []) {
			keys.add(p.identity_key ?? `name:${p.name}`);
		}
		for (const k of keys) if (!iconUrls[k]) fetchIcon(k);
	}

	function fmtMem(bytes) {
		const mb = bytes / (1024 * 1024);
		return mb >= 1024 ? `${(mb / 1024).toFixed(2)} GB` : `${mb.toFixed(1)} MB`;
	}
	function fmtBps(v) {
		if (v == null) return '—';
		const mb = v / (1024 * 1024);
		return mb >= 1 ? `${mb.toFixed(1)} MB/s` : `${(v / 1024).toFixed(0)} kB/s`;
	}
	const fmtPct = (v) => (v == null ? '—' : `${v.toFixed(1)} %`);
	function fmtClock(unix) {
		return new Date(unix * 1000).toLocaleTimeString('cs-CZ');
	}

	// Klik mimo graf ruší zámek → zpět na živá data. Výjimka: klik
	// v listu (řazení, scroll) zamčený čas NESMÍ zrušit.
	function onWindowClick(e) {
		if (
			pinned != null &&
			!e.target.closest('.chart-card') &&
			!e.target.closest('.table-card')
		) {
			pinned = null;
		}
	}

	onMount(() => {
		loadStatics();
		pollSystem();
		pollProcs();
		const t = setInterval(() => {
			pollSystem();
			pollProcs();
		}, 1000);
		return () => clearInterval(t);
	});

	const arrow = $derived(sortDir === -1 ? '↓' : '↑');
</script>

<svelte:window onclick={onWindowClick} />

<div class="tasks">
	<!-- ── Hlavní časový graf ── -->
	<section class="card chart-card">
		<header class="card-head">
			<div class="head-left">
				<span class="label-tech">// tasks / system</span>
				<div class="seg">
					{#each modes as m (m.id)}
						<button class:active={mode === m.id} onclick={() => (mode = m.id)}>
							{m.label}
						</button>
					{/each}
				</div>
			</div>
			<div class="readouts value-mono">
				{#if focusIdx != null}
					<span class="readout"><span class="k">{pinned != null ? '⌖ ČAS' : 'ČAS'}</span><span class="v accent w-time">{fmtClock(ts[focusIdx])}</span></span>
					<span class="readout" class:sel={mode === 'sys'}><span class="k">SYS</span><span class="v w-pct"><Num value={sys[focusIdx]} suffix=" %" /></span></span>
					<span class="readout" class:sel={mode === 'cpu'}><span class="k">CPU</span><span class="v w-pct"><Num value={cpu[focusIdx]} suffix=" %" /></span></span>
					<span class="readout" class:sel={mode === 'ram'}><span class="k">RAM</span><span class="v w-pct"><Num value={mem[focusIdx]} suffix=" %" /></span></span>
					<span class="readout" class:sel={mode === 'gpu'}><span class="k">GPU</span><span class="v w-pct"><Num value={gpu[focusIdx]} suffix=" %" /></span></span>
					<span class="readout" class:sel={mode === 'disk'}><span class="k">DISK</span><span class="v w-bps"><Num value={diskTot[focusIdx]} format={fmtBps} /></span></span>
					<span class="readout" class:sel={mode === 'net'}><span class="k">↓</span><span class="v net-down w-bps"><Num value={down[focusIdx]} format={fmtBps} /></span></span>
					<span class="readout" class:sel={mode === 'net'}><span class="k">↑</span><span class="v net-up w-bps"><Num value={up[focusIdx]} format={fmtBps} /></span></span>
				{:else if system}
					<span class="readout" class:sel={mode === 'sys'}><span class="k">SYS</span><span class="v w-pct"><Num value={sys.at(-1)} suffix=" %" /></span></span>
					<span class="readout" class:sel={mode === 'cpu'}><span class="k">CPU</span><span class="v w-pct"><Num value={system.cpu_pct} suffix=" %" /></span></span>
					<span class="readout" class:sel={mode === 'ram'}><span class="k">RAM</span><span class="v w-ram"><Num value={system.mem_used_mb / 1024} suffix=" GB" /> / {(system.mem_total_mb / 1024).toFixed(1)} GB</span></span>
					<span class="readout" class:sel={mode === 'gpu'}><span class="k">GPU</span><span class="v w-pct"><Num value={system.gpu_pct} suffix=" %" /></span></span>
					<span class="readout" class:sel={mode === 'disk'}><span class="k">DISK</span><span class="v w-bps"><Num value={diskTot.at(-1)} format={fmtBps} /></span></span>
					<span class="readout" class:sel={mode === 'net'}><span class="k">↓</span><span class="v net-down w-bps"><Num value={system.net_rx_bps} format={fmtBps} /></span></span>
					<span class="readout" class:sel={mode === 'net'}><span class="k">↑</span><span class="v net-up w-bps"><Num value={system.net_tx_bps} format={fmtBps} /></span></span>
					<span class="readout" title="Počet běžících procesů"><span class="k">PROCESY</span><span class="v w-cnt"><Num value={system.proc_count} decimals={0} /></span></span>
				{:else}
					<span class="readout"><span class="k">—</span></span>
				{/if}
			</div>
		</header>
		{#if mode === 'disk'}
			<!-- Per-disk grafy: read/write jako u sítě, každý disk zvlášť -->
			{#each statics?.disks ?? [] as d, di (d.index)}
				{@const s = diskSeries[d.index] ?? { ts: [], r: [], w: [] }}
				<div class="disk-block" class:first={di === 0}>
					<div class="disk-head">
						<span class="label-tech">disk {d.index} — {d.model}</span>
						<span class="value-mono disk-rates">
							<span class="net-down"><Num value={s.r.at(-1) ?? 0} format={fmtBps} /></span>
							<span class="net-up"><Num value={s.w.at(-1) ?? 0} format={fmtBps} /></span>
						</span>
					</div>
					<LiveChart
						ts={s.ts}
						values={s.r}
						values2={s.w}
						mode="net"
						labels={['čtení', 'zápis']}
						{pinned}
						onhover={(h) => (hover = h)}
						onpin={(t) => (pinned = t)}
					/>
				</div>
			{:else}
				<p class="err">{statics ? 'žádné disky' : 'čekám na info o discích…'}</p>
			{/each}
		{:else if daemon.alive || ts.length > 0}
			<LiveChart
				{ts}
				values={chartValues}
				values2={chartValues2}
				{mode}
				{pinned}
				onhover={(h) => (hover = h)}
				onpin={(t) => (pinned = t)}
			/>
		{:else}
			<p class="err">{error || 'služba neběží — graf čeká na data'}</p>
		{/if}
	</section>

	<!-- ── Detail vybrané proměnné (mezi grafem a listem) ── -->
	<section class="card detail-card">
		<header class="card-head">
			<span class="label-tech">
				// detail / {modes.find((m) => m.id === mode)?.label}{detailName
					? ` — ${detailName}`
					: ''}
			</span>
			{#if pinned != null}
				<span class="label-tech past-badge">⌖ {fmtClock(pinned)}</span>
			{/if}
		</header>

		{#if mode === 'sys'}
			<!-- Dlaždice všech proměnných, barva podle využití -->
			<div class="tiles">
				<div class="tile">
					<span class="tile-head"><Cpu size={16} strokeWidth={1.75} /> <span class="label-tech">cpu</span></span>
					<span class="tile-val value-mono" style:color={colorForLoad(dCpuPct)}><Num value={dCpuPct} suffix=" %" /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><MemoryStick size={16} strokeWidth={1.75} /> <span class="label-tech">ram</span></span>
					<span class="tile-val value-mono" style:color={colorForLoad(dMemPct)}><Num value={dMemPct} suffix=" %" /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><Zap size={16} strokeWidth={1.75} /> <span class="label-tech">gpu</span></span>
					<span class="tile-val value-mono" style:color={colorForLoad(dGpuPct)}><Num value={dGpuPct} suffix=" %" /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><HardDrive size={16} strokeWidth={1.75} /> <span class="label-tech">disk</span></span>
					<span class="tile-val value-mono"><Num value={dDiskTot} format={fmtBps} /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><ArrowDown size={16} strokeWidth={1.75} /> <span class="label-tech">down</span></span>
					<span class="tile-val value-mono" style:color="var(--net-down)"><Num value={dDown} format={fmtBps} /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><ArrowUp size={16} strokeWidth={1.75} /> <span class="label-tech">up</span></span>
					<span class="tile-val value-mono" style:color="var(--net-up)"><Num value={dUp} format={fmtBps} /></span>
				</div>
			</div>
		{:else if mode === 'cpu'}
			<!-- Jádra ve dvou sloupcích s mini grafy -->
			<div class="cores">
				{#each dCores as c, i (i)}
					<div class="core">
						<span class="label-tech core-name">C{i}</span>
						<span class="core-val value-mono" style:color={colorForLoad(c)}><Num value={c} decimals={0} suffix=" %" /></span>
						<div class="core-spark">
							<Sparkline
								values={pinned != null && pinnedCores
									? (pinnedCores.byCore[i] ?? [])
									: (coresHist[i] ?? [])}
								marker={pinned != null && pinnedCores ? pinnedCores.marker : null}
								height={22}
							/>
						</div>
					</div>
				{:else}
					<p class="empty-note label-tech">čekám na data jader…</p>
				{/each}
			</div>
			<!-- Doplňkové údaje (parita se Správcem úloh) -->
			<div class="tiles info-row">
				<div class="tile">
					<span class="label-tech">takt</span>
					<span class="tile-val value-mono"><Num value={system?.cpu_clock_mhz} decimals={0} suffix=" MHz" /></span>
				</div>
				<div class="tile">
					<span class="label-tech">základní takt</span>
					<span class="tile-val value-mono">{statics?.cpu_base_mhz ? `${statics.cpu_base_mhz} MHz` : '—'}</span>
				</div>
				<div class="tile">
					<span class="label-tech">jádra / vlákna</span>
					<span class="tile-val value-mono">{statics ? `${statics.physical_cores} / ${statics.logical_cores}` : '—'}</span>
				</div>
				<div class="tile">
					<span class="label-tech">cache L1/L2/L3</span>
					<span class="tile-val value-mono">
						{statics
							? `${(statics.l1_kb / 1024).toFixed(1)} / ${(statics.l2_kb / 1024).toFixed(0)} / ${(statics.l3_kb / 1024).toFixed(0)} MB`
							: '—'}
					</span>
				</div>
				<div class="tile">
					<span class="label-tech">vlákna celkem</span>
					<span class="tile-val value-mono"><Num value={system?.threads_total} decimals={0} /></span>
				</div>
				<div class="tile">
					<span class="label-tech">handly</span>
					<span class="tile-val value-mono"><Num value={system?.handles_total} decimals={0} /></span>
				</div>
				<div class="tile">
					<span class="label-tech">procesy</span>
					<span class="tile-val value-mono"><Num value={system?.proc_count} decimals={0} /></span>
				</div>
			</div>
		{:else if mode === 'ram'}
			<!-- Lineární využití + čísla + osazení modulů -->
			{#if system}
				{@const usedPct = dMemPct ?? 0}
				{@const usedGb = ((usedPct / 100) * system.mem_total_mb) / 1024}
				<div class="ram">
					<div class="ram-bar">
						<div
							class="ram-fill"
							style:width={`${usedPct}%`}
							style:background={colorForLoad(usedPct)}
						></div>
					</div>
					<!-- Vše v jednom řádku elementů (zalamuje se, nedělí se sekcemi) -->
					<div class="tiles">
						<div class="tile">
							<span class="label-tech">použito</span>
							<span class="tile-val value-mono" style:color={colorForLoad(usedPct)}><Num value={usedGb} suffix=" GB" /></span>
						</div>
						<div class="tile">
							<span class="label-tech">volno</span>
							<span class="tile-val value-mono"><Num value={system.mem_total_mb / 1024 - usedGb} suffix=" GB" /></span>
						</div>
						<div class="tile">
							<span class="label-tech">celkem</span>
							<span class="tile-val value-mono">{(system.mem_total_mb / 1024).toFixed(1)} GB</span>
						</div>
						<div class="tile">
							<span class="label-tech">využití</span>
							<span class="tile-val value-mono" style:color={colorForLoad(usedPct)}><Num value={usedPct} suffix=" %" /></span>
						</div>
						<div class="tile">
							<span class="label-tech">sloty</span>
							<span class="tile-val value-mono">
								{statics ? `${statics.ram_modules.length} / ${statics.ram_slots || '?'}` : '—'}
							</span>
						</div>
						<!-- Klíč = index: desky občas hlásí stejný slot u více modulů
						     a duplicitní klíč by shodil celý render. -->
						{#each statics?.ram_modules ?? [] as m, mi (mi)}
							<div class="tile">
								<span class="label-tech">{m.slot || `slot ${mi + 1}`}</span>
								<span class="tile-val value-mono">{(m.size_mb / 1024).toFixed(0)} GB</span>
								<span class="mod-sub label-tech">
									{m.configured_mts || m.speed_mts || '?'} MT/s
									{m.manufacturer ? ` · ${m.manufacturer}` : ''}
								</span>
							</div>
						{/each}
					</div>
				</div>
			{/if}
		{:else if mode === 'gpu'}
			{#if dGpu}
				<div class="tiles">
					<div class="tile">
						<span class="label-tech">teplota</span>
						<span class="tile-val value-mono" style:color={colorForTemp(dGpu.temp_c)}>
							<Num value={dGpu.temp_c} decimals={0} suffix=" °C" />
						</span>
					</div>
					<div class="tile">
						<span class="label-tech">vram</span>
						<span class="tile-val value-mono">
							{#if dGpu.vram_used_mb != null}
								<Num value={dGpu.vram_used_mb / 1024} suffix=" GB" /> / {((dGpu.vram_total_mb ?? system?.gpu?.vram_total_mb ?? 0) / 1024).toFixed(0)} GB
							{:else}—{/if}
						</span>
					</div>
					<div class="tile">
						<span class="label-tech">takt</span>
						<span class="tile-val value-mono"><Num value={dGpu.clock_mhz} decimals={0} suffix=" MHz" /></span>
					</div>
					<div class="tile">
						<span class="label-tech">spotřeba</span>
						<span class="tile-val value-mono"><Num value={dGpu.power_w} decimals={0} suffix=" W" /></span>
					</div>
					<div class="tile">
						<span class="label-tech">využití</span>
						<span class="tile-val value-mono" style:color={colorForLoad(dGpuPct)}><Num value={dGpuPct} suffix=" %" /></span>
					</div>
				</div>
			{:else}
				<p class="empty-note label-tech">gpu detail nedostupný — vyžaduje NVIDIA (NVML); AMD/Intel přijde ve v3</p>
			{/if}
		{:else if mode === 'disk'}
			<!-- Per-disk rychlosti (při zámku z historie) -->
			<div class="tiles">
				{#each dDisks as d (d.index)}
					{@const model = statics?.disks?.find((x) => x.index === d.index)?.model}
					<div class="tile">
						<span class="label-tech">disk {d.index}{model ? ` · ${model}` : ''}</span>
						<span class="tile-val value-mono">
							<span class="net-down"><Num value={d.r_bps} format={fmtBps} /></span>
							<span class="tile-sep">/</span>
							<span class="net-up"><Num value={d.w_bps} format={fmtBps} /></span>
						</span>
					</div>
				{:else}
					<p class="empty-note label-tech">žádná data disků…</p>
				{/each}
			</div>
		{:else}
			<!-- Síť: aktuální + špičky za session -->
			<div class="tiles">
				<div class="tile">
					<span class="tile-head"><ArrowDown size={16} strokeWidth={1.75} /> <span class="label-tech">aktuálně</span></span>
					<span class="tile-val value-mono" style:color="var(--net-down)"><Num value={dDown} format={fmtBps} /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><ArrowUp size={16} strokeWidth={1.75} /> <span class="label-tech">aktuálně</span></span>
					<span class="tile-val value-mono" style:color="var(--net-up)"><Num value={dUp} format={fmtBps} /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><ArrowDown size={16} strokeWidth={1.75} /> <span class="label-tech">špička</span></span>
					<span class="tile-val value-mono" style:color="var(--net-down)"><Num value={peakDown} format={fmtBps} /></span>
				</div>
				<div class="tile">
					<span class="tile-head"><ArrowUp size={16} strokeWidth={1.75} /> <span class="label-tech">špička</span></span>
					<span class="tile-val value-mono" style:color="var(--net-up)"><Num value={peakUp} format={fmtBps} /></span>
				</div>
			</div>
		{/if}
	</section>

	<!-- ── Tabulka procesů ── -->
	<section class="card table-card">
		<header class="card-head">
			<span class="label-tech">// processes</span>
			<div class="table-tools">
				{#if histProcs}
					<span class="label-tech past-badge">
						● stav z {fmtClock(histProcs.ts)} — zámek zrušíš klikem mimo graf
					</span>
				{/if}
				<input
					class="filter value-mono"
					type="text"
					placeholder="filtr (název / PID)…"
					bind:value={filter}
				/>
				<!-- Přepínač: seskupené aplikace / plochý seznam procesů -->
				<div class="seg">
					<button class:active={viewMode === 'apps'} onclick={() => (viewMode = 'apps')}>
						Aplikace
					</button>
					<button class:active={viewMode === 'procs'} onclick={() => (viewMode = 'procs')}>
						Procesy
					</button>
				</div>
			</div>
		</header>
		<div class="table-wrap">
			<table>
				<!-- Pevné šířky sloupců — hodnoty se neposouvají podle textu. -->
				<!-- name pevná, vydavatel jediný pružný (flex) → přisedne
				     blíž k názvu a má prostor i pro delší podpisy. -->
				<colgroup>
					<col style="width: 26px" />
					<col style="width: 240px" />
					<col />
					<col style="width: 66px" />
					<col style="width: 62px" />
					<col style="width: 62px" />
					<col style="width: 62px" />
					<col style="width: 92px" />
					<col style="width: 96px" />
					<col style="width: 62px" />
				</colgroup>
				<thead>
					<tr>
						<th class="t-dot" onclick={() => setSort('sys_pct')} title="Zátěž systému"></th>
						<th class="t-name" onclick={() => setSort('name')}>
							{viewMode === 'apps' ? 'Aplikace' : 'Proces'} {#if sortKey === 'name'}{arrow}{/if}
						</th>
						<th class="t-pub" onclick={() => setSort('publisher')}>
							Vydavatel {#if sortKey === 'publisher'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('pid')}>
							PID {#if sortKey === 'pid'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('sys_pct')}>
							Sys {#if sortKey === 'sys_pct'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('cpu_pct')}>
							CPU {#if sortKey === 'cpu_pct'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('gpu_pct')} title="Využití GPU procesem (PDH GPU Engine)">
							GPU {#if sortKey === 'gpu_pct'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('ws_bytes')}>
							Paměť {#if sortKey === 'ws_bytes'}{arrow}{/if}
						</th>
						<th
							class="t-num"
							onclick={() => setSort('disk_bps')}
							title="Veškeré I/O procesu (disk, pipe, zařízení) — jako Správce úloh v detailech"
						>
							I/O {#if sortKey === 'disk_bps'}{arrow}{/if}
						</th>
						<th class="t-num" onclick={() => setSort('threads')}>
							Vlákna {#if sortKey === 'threads'}{arrow}{/if}
						</th>
					</tr>
				</thead>
				<tbody>
					{#if viewMode === 'apps'}
						{#each groups as g (g.key)}
							{@const single = g.children.length === 1}
							{@const open = expanded.has(g.key)}
							<tr class="grp" class:clickable={!single} onclick={() => !single && toggleGroup(g.key)}>
								<td class="t-dot">
									<span
										class="load-dot"
										style:background={colorForLoad(g.sys_pct)}
										style:box-shadow={`0 0 5px ${colorForLoad(g.sys_pct)}`}
									></span>
								</td>
								<td class="t-name">
									<span class="tw">
										{#if iconUrls[g.key]}
											<img class="app-icon" src={iconUrls[g.key]} alt="" />
										{:else}
											<span class="app-icon placeholder"></span>
										{/if}
										<span class="app-name" class:guess={g.confidence === 'guess'}>{g.app_name}</span>
										{#if g.protection === 'critical'}
											<Lock class="lock-ico" size={13} strokeWidth={2} />
										{/if}
										{#if !single}
											<span class="count label-tech">{g.children.length}×</span>
											<span class="caret" class:open><ChevronRight size={13} strokeWidth={2.25} /></span>
										{/if}
									</span>
								</td>
								<td class="t-pub" title={g.publisher ?? ''}>
									{#if g.publisher}<span class="pub label-tech">{g.publisher}</span>{/if}
								</td>
								<td class="t-num value-mono">{single ? g.children[0].pid : ''}</td>
								<td class="t-num value-mono">{g.sys_pct.toFixed(1)} %</td>
								<td class="t-num value-mono">{g.cpu_pct.toFixed(1)} %</td>
								<td class="t-num value-mono">{g.gpu_pct > 0.05 ? `${g.gpu_pct.toFixed(1)} %` : '—'}</td>
								<td class="t-num value-mono">{fmtMem(g.ws_bytes)}</td>
								<td class="t-num value-mono">{g.disk_bps > 0 ? fmtBps(g.disk_bps) : '—'}</td>
								<td class="t-num value-mono">{single ? (g.children[0].threads ?? '—') : ''}</td>
							</tr>
							{#if !single && open}
								{#each g.children as p (p.pid)}
									<tr class="child" class:crit={p.protection === 'critical'}>
										<td class="t-dot"></td>
										<td class="t-name child-name">{p.name}</td>
										<td class="t-pub"></td>
										<td class="t-num value-mono">{p.pid}</td>
										<td class="t-num value-mono">{p.sys_pct.toFixed(1)} %</td>
										<td class="t-num value-mono">{p.cpu_pct.toFixed(1)} %</td>
										<td class="t-num value-mono">{p.gpu_pct > 0.05 ? `${p.gpu_pct.toFixed(1)} %` : '—'}</td>
										<td class="t-num value-mono">{fmtMem(p.ws_bytes)}</td>
										<td class="t-num value-mono">{p.disk_bps > 0 ? fmtBps(p.disk_bps) : '—'}</td>
										<td class="t-num value-mono">{p.threads ?? '—'}</td>
									</tr>
								{/each}
							{/if}
						{:else}
							<tr>
								<td colspan="10" class="empty label-tech">
									{daemon.alive ? 'čekám na první vzorek…' : 'služba neběží'}
								</td>
							</tr>
						{/each}
					{:else}
						<!-- Plochý seznam procesů (původní view) -->
						{#each visibleRows as p (p.pid)}
							<tr>
								<td class="t-dot">
									<span
										class="load-dot"
										style:background={colorForLoad(p.sys_pct)}
										style:box-shadow={`0 0 5px ${colorForLoad(p.sys_pct)}`}
									></span>
								</td>
								<td class="t-name">
									<span class="tw">
										{#if iconUrls[p.identity_key]}
											<img class="app-icon" src={iconUrls[p.identity_key]} alt="" />
										{:else}
											<span class="app-icon placeholder"></span>
										{/if}
										<span class="app-name">{p.name}</span>
										{#if p.protection === 'critical'}
											<Lock class="lock-ico" size={13} strokeWidth={2} />
										{/if}
									</span>
								</td>
								<td class="t-pub" title={p.publisher ?? ''}>
									{#if p.publisher}<span class="pub label-tech">{p.publisher}</span>{/if}
								</td>
								<td class="t-num value-mono">{p.pid}</td>
								<td class="t-num value-mono">{p.sys_pct.toFixed(1)} %</td>
								<td class="t-num value-mono">{p.cpu_pct.toFixed(1)} %</td>
								<td class="t-num value-mono">{p.gpu_pct > 0.05 ? `${p.gpu_pct.toFixed(1)} %` : '—'}</td>
								<td class="t-num value-mono">{fmtMem(p.ws_bytes)}</td>
								<td class="t-num value-mono">{p.disk_bps > 0 ? fmtBps(p.disk_bps) : '—'}</td>
								<td class="t-num value-mono">{p.threads ?? '—'}</td>
							</tr>
						{:else}
							<tr>
								<td colspan="10" class="empty label-tech">
									{daemon.alive ? 'čekám na první vzorek…' : 'služba neběží'}
								</td>
							</tr>
						{/each}
					{/if}
				</tbody>
			</table>
		</div>
	</section>
</div>

<style>
	.tasks {
		display: flex;
		flex-direction: column;
		gap: 1rem;
		/* min-height místo height: v režimu Disk je grafů víc a stránka
		   se poscrolluje — list procesů nesmí zkolabovat na nulu. */
		min-height: 100%;
	}
	.chart-card,
	.detail-card {
		flex-shrink: 0;
	}

	.card {
		background: var(--surface);
		border: 1px solid var(--border);
		border-radius: var(--radius-lg);
		padding: 0.9rem 1rem;
	}
	.card-head {
		display: flex;
		align-items: center;
		justify-content: space-between;
		margin-bottom: 0.7rem;
		min-height: 26px;
	}
	.head-left {
		display: flex;
		align-items: center;
		gap: 1rem;
	}

	.seg {
		display: inline-flex;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		overflow: hidden;
	}
	.seg button {
		border: 0;
		background: transparent;
		color: var(--text-faint);
		font-family: var(--font-mono);
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.06em;
		padding: 0.28rem 0.7rem;
		cursor: default;
	}
	.seg button:hover {
		color: var(--text-dim);
		background: var(--surface);
	}
	.seg button.active {
		color: var(--accent);
		background: var(--surface-hover);
	}

	.readouts {
		display: flex;
		gap: 1rem;
		font-size: 12px;
		flex-wrap: wrap;
		justify-content: flex-end;
	}
	.readout {
		padding: 0.15rem 0.4rem;
		border-radius: var(--radius-sm);
		border: 1px solid transparent;
	}
	.readout .k {
		color: var(--text-faint);
		margin-right: 0.35rem;
		font-size: 10.5px;
	}
	.readout .v {
		color: var(--text-dim);
		display: inline-block;
		text-align: right;
	}
	/* Zamčené šířky — hodnoty nemění pozici podle délky textu. */
	.v.w-pct {
		min-width: 4.4em;
	}
	.v.w-bps {
		min-width: 6.4em;
	}
	.v.w-ram {
		min-width: 9.5em;
	}
	.v.w-time {
		min-width: 5.2em;
	}
	.v.w-cnt {
		min-width: 2.6em;
	}
	/* Zvýraznění právě zobrazované proměnné grafu */
	.readout.sel {
		border-color: var(--border-strong);
		background: var(--surface-hover);
	}
	.readout.sel .v {
		color: var(--accent);
	}
	.readout.sel .k {
		color: var(--text-dim);
	}
	.readout .v.accent {
		color: var(--accent);
	}
	.readout .v.net-down {
		color: var(--net-down);
	}
	.readout .v.net-up {
		color: var(--net-up);
	}

	.err {
		margin: 0.6rem 0;
		color: var(--danger);
		font-size: 0.85rem;
	}
	.past-badge {
		color: var(--warn);
	}
	.table-tools {
		display: flex;
		align-items: center;
		gap: 1rem;
	}
	.filter {
		width: 190px;
		padding: 0.28rem 0.6rem;
		border: 1px solid var(--border);
		border-radius: var(--radius);
		background: var(--surface);
		color: var(--text);
		font-size: 11.5px;
		outline: none;
	}
	.filter:focus {
		border-color: var(--border-strong);
	}
	.filter::placeholder {
		color: var(--text-faint);
	}

	/* ── detail sekce ── */
	.detail-card {
		flex-shrink: 0;
	}
	.tiles {
		display: flex;
		gap: 0.7rem;
		flex-wrap: wrap;
	}
	.tile {
		display: flex;
		flex-direction: column;
		gap: 0.3rem;
		min-width: 128px;
		padding: 0.6rem 0.85rem;
		border: 1px dotted var(--border-strong);
		border-radius: var(--radius);
	}
	.tile-head {
		display: flex;
		align-items: center;
		gap: 0.4rem;
		color: var(--text-faint);
	}
	.tile-val {
		font-size: 1.12rem;
	}

	.cores {
		display: grid;
		grid-template-columns: 1fr 1fr;
		gap: 0.25rem 1.6rem;
		max-height: 190px;
		overflow-y: auto;
	}
	.core {
		display: flex;
		align-items: center;
		gap: 0.6rem;
	}
	.core-name {
		width: 30px;
		flex-shrink: 0;
	}
	.core-val {
		width: 52px;
		flex-shrink: 0;
		text-align: right;
		font-size: 0.82rem;
	}
	.core-spark {
		flex: 1;
		min-width: 0;
	}

	.ram-bar {
		height: 8px;
		border-radius: 4px;
		background: var(--surface-hover);
		overflow: hidden;
		margin-bottom: 0.8rem;
	}
	.ram-fill {
		height: 100%;
		border-radius: 4px;
	}

	.empty-note {
		margin: 0.3rem 0;
	}
	.info-row {
		margin-top: 0.8rem;
		padding-top: 0.8rem;
		border-top: 1px dotted var(--border-strong);
	}
	.mod-sub {
		text-transform: none;
		letter-spacing: 0.02em;
	}
	.tile-sep {
		color: var(--text-faint);
		margin: 0 0.2rem;
	}
	.net-down {
		color: var(--net-down);
	}
	.net-up {
		color: var(--net-up);
	}

	/* ── per-disk grafy (režim Disk) ── */
	.disk-block {
		margin-top: 0.9rem;
		padding-top: 0.7rem;
		border-top: 1px dotted var(--border-strong);
	}
	.disk-block.first {
		margin-top: 0;
		padding-top: 0;
		border-top: 0;
	}
	.disk-head {
		display: flex;
		align-items: baseline;
		justify-content: space-between;
		margin-bottom: 0.4rem;
	}
	/* Název disku jako nadpis — bílý text. */
	.disk-head > .label-tech {
		color: var(--text);
		font-size: 11.5px;
	}
	.disk-rates {
		display: flex;
		gap: 0.9rem;
		font-size: 12px;
	}

	/* ── tabulka (bez CSS přechodů — výkon při 200+ řádcích) ── */
	.table-card {
		flex: 1;
		display: flex;
		flex-direction: column;
		/* Vždy viditelný — i pod více disk grafy. */
		min-height: 280px;
		padding-bottom: 0;
	}
	.table-wrap {
		flex: 1;
		overflow-y: auto;
		min-height: 0;
		margin: 0 -1rem;
	}
	table {
		width: 100%;
		/* separate: bordery drží u buněk i při sticky hlavičce — jinak
		   se tečkovaná linka při scrollu na místech ztrácela */
		border-collapse: separate;
		border-spacing: 0;
		/* fixed + colgroup: šířky sloupců se nemění podle obsahu */
		table-layout: fixed;
		font-size: 0.86rem;
	}
	thead th {
		position: sticky;
		top: 0;
		background: #1a1b21;
		text-align: left;
		font-family: var(--font-mono);
		font-size: 10.5px;
		text-transform: uppercase;
		letter-spacing: 0.08em;
		font-weight: 400;
		color: var(--text-faint);
		padding: 0.45rem 1rem;
		white-space: nowrap;
	}
	thead th:hover {
		color: var(--text-dim);
	}
	td {
		padding: 0.34rem 1rem;
		border-bottom: 1px solid var(--border);
		color: var(--text-dim);
		white-space: nowrap;
	}
	tbody tr:hover td {
		background: var(--surface);
		color: var(--text);
	}
	.t-dot {
		width: 26px;
		padding-right: 0 !important;
	}

	/* ── strom aplikace → procesy (v2) ── */
	.grp.clickable {
		cursor: pointer;
	}
	.grp td.t-name {
		color: var(--text);
	}
	.tw {
		display: inline-flex;
		align-items: center;
		gap: 0.45rem;
		min-width: 0;
	}
	/* Caret až za názvem — rozbalovací šipka aplikace. */
	.caret {
		display: inline-flex;
		align-items: center;
		color: var(--text-faint);
		transition: transform 130ms ease-out;
		flex-shrink: 0;
	}
	.caret.open {
		transform: rotate(90deg);
	}
	/* Ikona aplikace extrahovaná z .exe. */
	.app-icon {
		width: 16px;
		height: 16px;
		flex-shrink: 0;
		object-fit: contain;
		image-rendering: -webkit-optimize-contrast;
	}
	/* Řádek aplikace (jen seskupený) — o něco větší ikona i název. */
	.grp .app-icon {
		width: 22px;
		height: 22px;
	}
	.grp .app-name {
		font-size: 1.02rem;
	}
	.app-icon.placeholder {
		border-radius: 3px;
		background: var(--surface-hover);
	}
	.app-name {
		font-weight: 500;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	/* Guess/path identita — tečkovaný podtrh (SPEC 4.4). */
	.app-name.guess {
		text-decoration: underline dotted var(--text-faint);
		text-underline-offset: 3px;
	}
	.count {
		color: var(--text-faint);
		flex-shrink: 0;
	}
	/* Vydavatel — vlastní sloupec, zarovnaný napříč řádky. */
	.t-pub {
		overflow: hidden;
		white-space: nowrap;
		text-overflow: ellipsis;
	}
	.pub {
		color: var(--text-faint);
		text-transform: none;
		letter-spacing: 0.01em;
		opacity: 0.8;
	}
	/* Zámek kritického procesu — Lucide ikona, žlutě. */
	:global(.lock-ico) {
		color: var(--warn);
		flex-shrink: 0;
	}
	/* Kritické procesy: šedě (SPEC 9.4). */
	.grp .app-name,
	.child.crit td {
		color: var(--text);
	}
	tr.child td {
		background: rgba(255, 255, 255, 0.012);
	}
	.child-name {
		padding-left: 2.2rem !important;
		color: var(--text-dim) !important;
	}
	.child.crit .child-name {
		color: var(--text-faint) !important;
	}
	.load-dot {
		display: inline-block;
		width: 7px;
		height: 7px;
		border-radius: 50%;
		vertical-align: middle;
	}
	.t-name {
		width: 100%;
		max-width: 0;
		overflow: hidden;
		text-overflow: ellipsis;
	}
	td.t-name {
		color: var(--text);
	}
	.t-num {
		text-align: right;
	}
	th.t-num {
		text-align: right;
	}
	.empty {
		text-align: center;
		padding: 2rem 0;
	}
</style>
