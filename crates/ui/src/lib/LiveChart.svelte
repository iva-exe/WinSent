<script>
	// Živý časový graf (uPlot, DESIGN.md kap. 7).
	//
	// Režimy: 'cpu' / 'ram' = jedna proměnná s gradientem podle zátěže
	// (zelená → jantar → červená, gradient v barvě křivky je povolený),
	// 'all' = kombinovaný graf systému (CPU bílá = akcent, RAM tlumená).
	// Hover: `onhover` dostává hodnoty z času pod kurzorem, nebo null.
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';
	import { onMount } from 'svelte';

	let { ts = [], cpu = [], mem = [], mode = 'all', onhover = () => {} } = $props();

	let el;
	let u;

	function cssVar(name) {
		return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
	}

	function hexToRgba(hex, alpha) {
		const v = hex.replace('#', '');
		const n = parseInt(v.length === 3 ? v.replace(/./g, (c) => c + c) : v, 16);
		return `rgba(${(n >> 16) & 255}, ${(n >> 8) & 255}, ${n & 255}, ${alpha})`;
	}

	// Gradient podle zátěže přes osu y (0–100 %): do ~55 % zelená,
	// kolem 75 % jantar, od ~90 % červená.
	function loadGradient(uu, alpha) {
		const ok = cssVar('--ok') || '#4ade80';
		const warn = cssVar('--warn') || '#f59e0b';
		const danger = cssVar('--danger') || '#ef4444';
		const yMin = uu.valToPos(0, 'y', true);
		const yMax = uu.valToPos(100, 'y', true);
		const g = uu.ctx.createLinearGradient(0, yMin, 0, yMax);
		g.addColorStop(0, hexToRgba(ok, alpha));
		g.addColorStop(0.55, hexToRgba(ok, alpha));
		g.addColorStop(0.75, hexToRgba(warn, alpha));
		g.addColorStop(0.9, hexToRgba(danger, alpha));
		g.addColorStop(1, hexToRgba(danger, alpha));
		return g;
	}

	function buildSeries() {
		const accent = cssVar('--accent') || '#ffffff';
		const dim = cssVar('--text-dim') || '#9a9aa1';
		if (mode === 'all') {
			return [
				{},
				{ label: 'CPU', stroke: accent, width: 1.6, fill: 'rgba(255,255,255,0.05)' },
				{ label: 'RAM', stroke: dim, width: 1.2 }
			];
		}
		// Jedna proměnná — gradient podle zátěže v tahu i jemné výplni.
		return [
			{},
			{
				label: mode.toUpperCase(),
				width: 1.8,
				stroke: (uu) => loadGradient(uu, 1),
				fill: (uu) => loadGradient(uu, 0.07)
			}
		];
	}

	function chartData() {
		if (mode === 'cpu') return [ts, cpu];
		if (mode === 'ram') return [ts, mem];
		return [ts, cpu, mem];
	}

	function build() {
		const faint = cssVar('--text-faint') || '#5c5c63';
		const grid = 'rgba(255,255,255,0.07)';
		u?.destroy();
		u = new uPlot(
			{
				width: el.clientWidth,
				height: 240,
				padding: [12, 10, 0, 0],
				legend: { show: false },
				cursor: {
					points: { show: true, size: 5 },
					y: false
				},
				scales: { y: { range: [0, 100] } },
				axes: [
					{
						stroke: faint,
						font: '10px "Fira Mono", monospace',
						ticks: { show: false },
						grid: { show: false }
					},
					{
						stroke: faint,
						font: '10px "Fira Mono", monospace',
						size: 42,
						ticks: { show: false },
						grid: { stroke: grid, width: 1, dash: [2, 4] },
						values: (_u, vals) => vals.map((v) => `${v} %`)
					}
				],
				series: buildSeries(),
				hooks: {
					setCursor: [
						(uu) => {
							const i = uu.cursor.idx;
							if (i == null || ts[i] == null) {
								onhover(null);
							} else {
								onhover({ t: ts[i], cpu: cpu[i], mem: mem[i] });
							}
						}
					]
				}
			},
			chartData(),
			el
		);
	}

	onMount(() => {
		build();
		const ro = new ResizeObserver(() => {
			if (el && u) u.setSize({ width: el.clientWidth, height: 240 });
		});
		ro.observe(el);
		return () => {
			ro.disconnect();
			u?.destroy();
		};
	});

	// Změna režimu = jiné série → graf se staví znovu.
	$effect(() => {
		mode;
		if (u && el) build();
	});

	// Nová data jen doplní body.
	$effect(() => {
		if (u) u.setData(chartData());
	});
</script>

<div bind:this={el} class="chart"></div>

<style>
	.chart {
		width: 100%;
	}
	.chart :global(.u-over) {
		cursor: crosshair;
	}
</style>
