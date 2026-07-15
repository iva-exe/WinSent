<script>
	// Živý časový graf s historií (uPlot, DESIGN.md kap. 7).
	//
	// Interakce:
	//  • kolečko        = posun v čase (dolů = do minulosti); u přítomnosti
	//                     se přisnapne a sleduje živá data — v minulosti se
	//                     view NEHÝBE, i když přitékají nová data
	//  • Ctrl+kolečko   = zoom (30 s – 1 h)
	//  • klik do grafu  = zámek na bod v čase (nezávislý na myši);
	//                     přepíše se dalším klikem, ruší se klikem mimo graf
	//  • hover          = linka + dutá tečka s borderem v barvě zátěže,
	//                     hodnoty hlásí `onhover`
	// Indikátory: spodní čára = pozice okna v historii (skrytá na živě),
	// pravá čára = zoom (střed výšky = odzoomováno, plná = přizoomováno;
	// skrytá v defaultu 180 s).
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';
	import { onMount } from 'svelte';

	let {
		ts = [],
		values = [],
		mode = 'sys',
		pinned = null,
		onhover = () => {},
		onpin = () => {}
	} = $props();

	const SPAN_DEFAULT = 180;
	const SPAN_MIN = 30;
	const SPAN_MAX = 3600;

	let el;
	let u;
	let pinEl;

	// View: span = viditelné sekundy, endTs = pravý okraj (null = živě).
	let span = $state(SPAN_DEFAULT);
	let endTs = $state(null);

	// Indikátory (odvozené při každém překreslení).
	let panFrac = $state({ left: 0, width: 1 });
	const isPercent = $derived(mode !== 'down' && mode !== 'up');
	const follow = $derived(endTs === null);
	const zoomT = $derived(
		(Math.log(SPAN_MAX) - Math.log(span)) / (Math.log(SPAN_MAX) - Math.log(SPAN_MIN))
	);

	function cssVar(name) {
		return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
	}

	function hexToRgb(hex) {
		const v = hex.replace('#', '');
		const n = parseInt(v.length === 3 ? v.replace(/./g, (c) => c + c) : v, 16);
		return [(n >> 16) & 255, (n >> 8) & 255, n & 255];
	}
	const rgba = ([r, g, b], a) => `rgba(${r}, ${g}, ${b}, ${a})`;
	const lerp = (c1, c2, t) => c1.map((v, i) => Math.round(v + (c2[i] - v) * t));

	// Barva pro hodnotu zátěže (0–100) — stejné zastávky jako gradient.
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

	function fmtBps(v) {
		if (v == null) return '—';
		const mb = v / (1024 * 1024);
		if (mb >= 1) return `${mb.toFixed(1)} MB/s`;
		return `${(v / 1024).toFixed(0)} kB/s`;
	}

	function lastTs() {
		return ts.length ? ts[ts.length - 1] : null;
	}

	// Nastaví osu x podle view a přepočítá indikátory.
	function applyScale() {
		const last = lastTs();
		if (!u || last == null) return;
		const end = endTs ?? last;
		u.setScale('x', { min: end - span, max: end });

		const histStart = last - SPAN_MAX;
		panFrac = {
			left: Math.max(0, (end - span - histStart) / SPAN_MAX),
			width: Math.min(1, span / SPAN_MAX)
		};
		positionPin();
	}

	// Svislá linka zámku — overlay v CSS souřadnicích plochy grafu.
	function positionPin() {
		if (!u || !pinEl) return;
		if (pinned == null) {
			pinEl.style.display = 'none';
			return;
		}
		const x = u.valToPos(pinned, 'x', false);
		if (x < 0 || x > u.over.clientWidth) {
			pinEl.style.display = 'none';
			return;
		}
		pinEl.style.display = 'block';
		pinEl.style.left = `${x}px`;
	}

	function onWheel(e) {
		e.preventDefault();
		const last = lastTs();
		if (last == null) return;
		if (e.ctrlKey) {
			// Zoom kolem pravého okraje.
			const f = e.deltaY > 0 ? 1.25 : 0.8;
			span = Math.round(Math.min(SPAN_MAX, Math.max(SPAN_MIN, span * f)));
		} else {
			// Pan: dolů = do minulosti, nahoru = k přítomnosti.
			const step = span * 0.12;
			let end = (endTs ?? last) + (e.deltaY > 0 ? -step : step);
			const first = ts.length ? ts[0] : last;
			end = Math.max(Math.min(end, last), Math.min(first + span, last));
			// Snap na přítomnost → follow režim.
			endTs = end >= last - 0.5 ? null : end;
		}
		applyScale();
		u?.redraw();
	}

	function onClick() {
		const i = u?.cursor.idx;
		if (i != null && ts[i] != null) {
			onpin(ts[i]);
		}
	}

	function build() {
		const faint = cssVar('--text-faint') || '#5c5c63';
		const accent = cssVar('--accent') || '#ffffff';
		const grid = 'rgba(255,255,255,0.07)';
		u?.destroy();
		u = new uPlot(
			{
				width: el.clientWidth,
				height: 240,
				padding: [12, 10, 0, 0],
				legend: { show: false },
				cursor: {
					x: true,
					y: false,
					drag: { x: false, y: false },
					points: {
						show: true,
						size: 8,
						width: 1.6,
						fill: () => 'rgba(0,0,0,0)',
						stroke: (uu, sidx) => {
							const i = uu.cursor.idx;
							const v = i != null ? uu.data[sidx]?.[i] : null;
							return isPercent ? colorForLoad(v) : accent;
						}
					}
				},
				scales: {
					y: isPercent
						? { range: [0, 100] }
						: {
								range: (_u, _min, max) => [0, Math.max((max ?? 0) * 1.15, 1024 * 1024)]
							}
				},
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
						size: isPercent ? 42 : 64,
						ticks: { show: false },
						grid: { stroke: grid, width: 1, dash: [2, 4] },
						values: (_u, vals) =>
							vals.map((v) => (isPercent ? `${v} %` : fmtBps(v)))
					}
				],
				series: [
					{},
					isPercent
						? {
								width: 1.8,
								stroke: (uu) => loadGradient(uu, 1),
								fill: (uu) => loadGradient(uu, 0.07)
							}
						: {
								width: 1.6,
								stroke: accent,
								fill: 'rgba(255,255,255,0.05)'
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
					],
					draw: [() => positionPin()]
				}
			},
			[ts, values],
			el
		);

		// Overlay pro zámek + interakce na ploše grafu.
		pinEl = document.createElement('div');
		pinEl.className = 'pin-line';
		pinEl.style.display = 'none';
		u.over.appendChild(pinEl);
		u.over.addEventListener('wheel', onWheel, { passive: false });
		u.over.addEventListener('click', onClick);
		applyScale();
	}

	onMount(() => {
		build();
		const ro = new ResizeObserver(() => {
			if (el && u) {
				u.setSize({ width: el.clientWidth, height: 240 });
				applyScale();
			}
		});
		ro.observe(el);
		return () => {
			ro.disconnect();
			u?.destroy();
		};
	});

	// Změna metriky → nový graf (jiná osa/škála/gradient).
	$effect(() => {
		mode;
		if (u && el) build();
	});

	// Nová data: doplnit body BEZ resetu os; na živě posunout okno.
	$effect(() => {
		if (!u) return;
		u.setData([ts, values], false);
		applyScale();
	});

	// Zámek zvenku (zrušení klikem mimo graf) → překreslit linku.
	$effect(() => {
		pinned;
		positionPin();
	});
</script>

<div class="wrap">
	<div bind:this={el} class="chart"></div>

	<!-- Zoom indikátor: střed = odzoomováno, plná výška = přizoomováno -->
	<div
		class="zoom-ind"
		class:hidden={span === SPAN_DEFAULT}
		style:height={`${50 + 50 * zoomT}%`}
	></div>

	<!-- Pan indikátor: pozice okna v hodinové historii (skrytý na živě) -->
	<div class="pan-ind" class:hidden={follow}>
		<div
			class="thumb"
			style:left={`${panFrac.left * 100}%`}
			style:width={`${Math.max(panFrac.width * 100, 2)}%`}
		></div>
	</div>
</div>

<style>
	.wrap {
		position: relative;
		width: 100%;
		padding-right: 10px;
	}
	.chart {
		width: 100%;
	}
	.chart :global(.u-over) {
		cursor: crosshair;
	}
	/* Linka zámku času — drží pozici nezávisle na myši. */
	.chart :global(.pin-line) {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 0;
		border-left: 1px solid rgba(255, 255, 255, 0.55);
		pointer-events: none;
	}

	/* Ukazatele: lehce průhledné bílé čáry, v defaultu neviditelné. */
	.zoom-ind {
		position: absolute;
		right: 2px;
		top: 50%;
		transform: translateY(-50%);
		width: 3px;
		border-radius: 2px;
		background: rgba(255, 255, 255, 0.28);
		transition: opacity 200ms ease-out, height 120ms ease-out;
	}
	.pan-ind {
		position: relative;
		height: 3px;
		margin-top: 4px;
		border-radius: 2px;
		background: rgba(255, 255, 255, 0.06);
		transition: opacity 200ms ease-out;
	}
	.pan-ind .thumb {
		position: absolute;
		top: 0;
		bottom: 0;
		border-radius: 2px;
		background: rgba(255, 255, 255, 0.28);
		transition: left 80ms linear, width 120ms ease-out;
	}
	.hidden {
		opacity: 0;
	}
</style>
