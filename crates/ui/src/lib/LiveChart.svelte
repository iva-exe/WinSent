<script>
	// Živý časový graf (uPlot, DESIGN.md kap. 7).
	//
	// Vždy JEDNA čára s gradientem podle zátěže (zelená → jantar →
	// červená). Režimy: 'cpu', 'ram', 'all' (kombinovaná zátěž systému —
	// váhu má nejvytíženější komponenta, viz tasks/+page.svelte).
	// Cursor: vertikální linka zůstává tam, kde se myš zastavila, a dutá
	// tečka s borderem v barvě zátěže daného místa; `onhover` hlásí
	// hodnoty z času pod kurzorem, null po opuštění grafu.
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';
	import { onMount } from 'svelte';

	let { ts = [], values = [], mode = 'all', onhover = () => {} } = $props();

	let el;
	let u;

	function cssVar(name) {
		return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
	}

	function hexToRgb(hex) {
		const v = hex.replace('#', '');
		const n = parseInt(v.length === 3 ? v.replace(/./g, (c) => c + c) : v, 16);
		return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
	}

	function rgba([r, g, b], a) {
		return `rgba(${r}, ${g}, ${b}, ${a})`;
	}

	function lerp(c1, c2, t) {
		return c1.map((v, i) => Math.round(v + (c2[i] - v) * t));
	}

	// Barva pro konkrétní hodnotu zátěže (0–100) — stejné zastávky jako
	// gradient křivky: do 55 zelená, 75 jantar, od 90 červená.
	function colorForLoad(v) {
		const ok = hexToRgb(cssVar('--ok') || '#4ade80');
		const warn = hexToRgb(cssVar('--warn') || '#f59e0b');
		const danger = hexToRgb(cssVar('--danger') || '#ef4444');
		if (v == null) return rgba(ok, 1);
		if (v <= 55) return rgba(ok, 1);
		if (v <= 75) return rgba(lerp(ok, warn, (v - 55) / 20), 1);
		if (v <= 90) return rgba(lerp(warn, danger, (v - 75) / 15), 1);
		return rgba(danger, 1);
	}

	// Gradient podle zátěže přes osu y (0–100 %).
	function loadGradient(uu, alpha) {
		const ok = hexToRgb(cssVar('--ok') || '#4ade80');
		const warn = hexToRgb(cssVar('--warn') || '#f59e0b');
		const danger = hexToRgb(cssVar('--danger') || '#ef4444');
		const yMin = uu.valToPos(0, 'y', true);
		const yMax = uu.valToPos(100, 'y', true);
		const g = uu.ctx.createLinearGradient(0, yMin, 0, yMax);
		g.addColorStop(0, rgba(ok, alpha));
		g.addColorStop(0.55, rgba(ok, alpha));
		g.addColorStop(0.75, rgba(warn, alpha));
		g.addColorStop(0.9, rgba(danger, alpha));
		g.addColorStop(1, rgba(danger, alpha));
		return g;
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
					// Vertikální linka kurzoru — zůstává, dokud se myš
					// nepohne nebo neopustí graf (výchozí chování uPlot).
					x: true,
					y: false,
					points: {
						show: true,
						size: 8,
						width: 1.6,
						// Dutá tečka: průhledná výplň, border v barvě
						// zátěže hodnoty pod kurzorem.
						fill: () => 'rgba(0,0,0,0)',
						stroke: (uu, sidx) => {
							const i = uu.cursor.idx;
							const v = i != null ? uu.data[sidx]?.[i] : null;
							return colorForLoad(v);
						}
					}
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
				series: [
					{},
					{
						label: mode.toUpperCase(),
						width: 1.8,
						stroke: (uu) => loadGradient(uu, 1),
						fill: (uu) => loadGradient(uu, 0.07)
					}
				],
				hooks: {
					setCursor: [
						(uu) => {
							const i = uu.cursor.idx;
							if (i == null || ts[i] == null) {
								onhover(null);
							} else {
								onhover({ t: ts[i], v: values[i], i });
							}
						}
					]
				}
			},
			[ts, values],
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

	// Změna režimu = nový popisek série → graf se staví znovu.
	$effect(() => {
		mode;
		if (u && el) build();
	});

	// Nová data jen doplní body.
	$effect(() => {
		if (u) u.setData([ts, values]);
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
	/* Vertikální linka kurzoru — jemná, tečkovaná (industrial). */
	.chart :global(.u-cursor-x) {
		border-right: 1px dotted var(--border-strong);
	}
</style>
