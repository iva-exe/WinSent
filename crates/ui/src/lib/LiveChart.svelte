<script>
	// Živý časový graf s historií (uPlot, DESIGN.md kap. 7).
	//
	// Interakce:
	//  • kolečko        = posun v čase (dolů = do minulosti); u přítomnosti
	//                     snap na živě — v minulosti se view nehýbe
	//  • Ctrl+kolečko   = zoom (30 s – 1 h)
	//  • dvojklik       = přiblížit na místo pod kurzorem
	//  • klik na šipku  = přejít na incident v Incidents
	//  • klik do grafu  = zámek na bod v čase (linka + tečka na křivce,
	//                     nezávislé na myši); přepíše se dalším klikem,
	//                     ruší se klikem mimo graf
	//  • hover          = linka + dutá tečka s borderem v barvě zátěže
	// Režimy: procentní (cpu/ram/gpu/sys, gradient dle zátěže) a 'net'
	// (dvě série: download --net-down, upload --net-up, dynamická osa).
	import uPlot from 'uplot';
	import 'uplot/dist/uPlot.min.css';
	import { onMount, untrack } from 'svelte';

	let {
		ts = [],
		values = [],
		values2 = null,
		mode = 'sys',
		pinned = null,
		labels = ['download', 'upload'],
		// Incidenty na časové ose: [{ ts, kind }] — trojúhelníček dole
		// (zásek žlutě, pád/BSOD červeně) + tečkovaná svislice.
		markers = [],
		onhover = () => {},
		onpin = () => {},
		// Klik na šipku incidentu na časové ose.
		onmarker = () => {}
	} = $props();

	const SPAN_DEFAULT = 180;
	const SPAN_MIN = 30;
	const SPAN_MAX = 86400; // 24 h — osa vždy pokrývá poslední den

	let el;
	let u;
	let pinLineEl;
	let pinDotEl;
	let pinLabelEl;

	let span = $state(SPAN_DEFAULT);
	let endTs = $state(null);

	let panFrac = $state({ left: 0, width: 1 });
	const isNet = $derived(mode === 'net');
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

	const lastTs = () => (ts.length ? ts[ts.length - 1] : null);

	function chartData() {
		return isNet ? [ts, values, values2 ?? []] : [ts, values];
	}

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

	// Zámek: svislá linka + tečka na křivce + štítek s časem.
	function positionPin() {
		if (!u || !pinLineEl) return;
		if (pinned == null) {
			pinLineEl.style.display = 'none';
			pinDotEl.style.display = 'none';
			pinLabelEl.style.display = 'none';
			return;
		}
		const x = u.valToPos(pinned, 'x', false);
		if (x < 0 || x > u.over.clientWidth) {
			pinLineEl.style.display = 'none';
			pinDotEl.style.display = 'none';
			pinLabelEl.style.display = 'none';
			return;
		}
		pinLineEl.style.display = 'block';
		pinLineEl.style.left = `${x}px`;

		// Štítek s časem zámku — přilepený k lince, u krajů se přetočí.
		pinLabelEl.style.display = 'block';
		pinLabelEl.textContent = `⌖ ${new Date(pinned * 1000).toLocaleTimeString('cs-CZ')}`;
		const flip = x > u.over.clientWidth - 78;
		pinLabelEl.style.left = `${x}px`;
		pinLabelEl.style.transform = flip ? 'translateX(calc(-100% - 6px))' : 'translateX(6px)';

		// Tečka na hodnotě primární série (u sítě na downloadu).
		const i = nearestIdx(pinned);
		const v = i != null ? values[i] : null;
		if (v == null) {
			pinDotEl.style.display = 'none';
			return;
		}
		const y = u.valToPos(v, 'y', false);
		pinDotEl.style.display = 'block';
		pinDotEl.style.left = `${x}px`;
		pinDotEl.style.top = `${y}px`;
		pinDotEl.style.borderColor = isNet
			? cssVar('--net-down') || '#7cc0ff'
			: colorForLoad(v);
	}

	// Šrafování úseků bez měření: mezera > 30 s mezi vzorky (kaskáda
	// dává max 10s buckety uvnitř dne) = služba neběžela.
	let hatchPat = null;
	function hatchPattern(ctx, dpr) {
		if (!hatchPat) {
			const c = document.createElement('canvas');
			const s = Math.round(8 * dpr);
			c.width = s;
			c.height = s;
			const g = c.getContext('2d');
			g.strokeStyle = 'rgba(255,255,255,0.07)';
			g.lineWidth = dpr;
			g.beginPath();
			g.moveTo(-2, s + 2);
			g.lineTo(s + 2, -2);
			g.stroke();
			hatchPat = ctx.createPattern(c, 'repeat');
		}
		return hatchPat;
	}

	function lowerBound(arr, v) {
		let lo = 0;
		let hi = arr.length;
		while (lo < hi) {
			const mid = (lo + hi) >> 1;
			if (arr[mid] < v) lo = mid + 1;
			else hi = mid;
		}
		return lo;
	}

	function drawGaps(uu) {
		if (!ts.length) return;
		const ctx = uu.ctx;
		const { left, top, width, height } = uu.bbox;
		const dpr = window.devicePixelRatio || 1;
		const xMin = uu.scales.x.min;
		const xMax = uu.scales.x.max;
		const gaps = [];
		if (ts[0] > xMin + 1) gaps.push([xMin, ts[0]]);
		let i = Math.max(0, lowerBound(ts, xMin) - 1);
		for (; i < ts.length - 1 && ts[i] <= xMax; i++) {
			if (ts[i + 1] - ts[i] > 30) gaps.push([ts[i], ts[i + 1]]);
		}
		if (ts[ts.length - 1] < xMax - 30) gaps.push([ts[ts.length - 1], xMax]);
		if (!gaps.length) return;
		ctx.save();
		ctx.fillStyle = hatchPattern(ctx, dpr);
		for (const [a, b] of gaps) {
			const x1 = Math.max(left, uu.valToPos(Math.max(a, xMin), 'x', true));
			const x2 = Math.min(left + width, uu.valToPos(Math.min(b, xMax), 'x', true));
			if (x2 - x1 > 2) {
				ctx.fillRect(x1, top, x2 - x1, height);
			}
		}
		ctx.restore();
	}

	// Markery incidentů — kreslí se přímo do canvasu grafu (draw hook),
	// takže drží pozici při pan/zoomu bez DOM overlaye.
	function drawMarkers(uu) {
		if (!markers.length) return;
		const ctx = uu.ctx;
		const { left, top, width, height } = uu.bbox;
		const dpr = window.devicePixelRatio || 1;
		const warn = cssVar('--warn') || '#f59e0b';
		const danger = cssVar('--danger') || '#ef4444';
		ctx.save();
		for (const m of markers) {
			const x = uu.valToPos(m.ts, 'x', true);
			if (x < left || x > left + width) continue;
			const color = m.kind === 'stall' ? warn : danger;
			ctx.strokeStyle = color;
			ctx.globalAlpha = 0.3;
			ctx.lineWidth = dpr;
			ctx.setLineDash([2 * dpr, 4 * dpr]);
			ctx.beginPath();
			ctx.moveTo(x, top);
			ctx.lineTo(x, top + height);
			ctx.stroke();
			ctx.setLineDash([]);
			ctx.globalAlpha = 1;
			const s = 5 * dpr;
			ctx.fillStyle = color;
			ctx.beginPath();
			ctx.moveTo(x, top + height - s);
			ctx.lineTo(x - s * 0.8, top + height);
			ctx.lineTo(x + s * 0.8, top + height);
			ctx.closePath();
			ctx.fill();
		}
		ctx.restore();
	}

	function nearestIdx(t) {
		if (!ts.length) return null;
		// binární hledání nejbližšího času
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

	// ── Pan/zoom s rAF throttlingem: wheel eventy chodí rychleji než
	// stíhá překreslení; deltu akumulujeme a aplikujeme 1× za snímek.
	let pendingPan = 0;
	let pendingZoom = 0;
	let rafId = null;

	function setEnd(end) {
		const last = lastTs();
		if (last == null) return;
		// Osa reflektuje posledních 24 h — scrollovat jde i do úseků,
		// kde se neměřilo (ty jsou vyšrafované).
		end = Math.max(Math.min(end, last), last - SPAN_MAX + span);
		endTs = end >= last - 0.5 ? null : end;
	}

	function flushView() {
		rafId = null;
		const last = lastTs();
		if (last == null) return;
		if (pendingZoom !== 0) {
			const f = Math.pow(1.25, pendingZoom);
			span = Math.round(Math.min(SPAN_MAX, Math.max(SPAN_MIN, span * f)));
			pendingZoom = 0;
		}
		if (pendingPan !== 0) {
			setEnd((endTs ?? last) + pendingPan);
			pendingPan = 0;
		}
		applyScale();
	}

	function scheduleFlush() {
		if (rafId == null) rafId = requestAnimationFrame(flushView);
	}

	function onWheel(e) {
		e.preventDefault();
		if (e.ctrlKey) {
			pendingZoom += e.deltaY > 0 ? 1 : -1;
		} else {
			pendingPan += span * 0.12 * (e.deltaY > 0 ? -1 : 1);
		}
		scheduleFlush();
	}

	// Pan tažením: držení kolečka (prostřední tlačítko) na grafu.
	let dragging = false;

	function onMouseDown(e) {
		if (e.button !== 1) return;
		e.preventDefault(); // vypnout autoscroll Windows
		dragging = true;
	}

	function onMouseMove(e) {
		if (!dragging || !u) return;
		const secPerPx = span / Math.max(u.over.clientWidth, 1);
		pendingPan -= e.movementX * secPerPx;
		scheduleFlush();
	}

	function onMouseUp() {
		dragging = false;
	}

	// Marker pod kurzorem: šipka dole je terč zhruba 9 px na každou
	// stranu. Hledá se nejbližší, ne první — při přiblížení jich vedle
	// sebe může být víc.
	function markerAt(clientX, clientY) {
		if (!u || !markers.length) return null;
		const rect = u.over.getBoundingClientRect();
		const x = clientX - rect.left;
		const y = clientY - rect.top;
		// Terč je spodní třetina grafu, kde šipka sedí.
		if (y < rect.height - 22) return null;
		let best = null;
		let bestD = 10;
		for (const m of markers) {
			const mx = u.valToPos(m.ts, 'x', false);
			const d = Math.abs(mx - x);
			if (d < bestD) {
				bestD = d;
				best = m;
			}
		}
		return best;
	}

	function onClick(e) {
		if (!u) return;
		// Klik na šipku incidentu je odkaz, ne zámek na bod v čase.
		const m = markerAt(e.clientX, e.clientY);
		if (m) {
			onmarker(m);
			return;
		}
		// Čas z pozice kliknutí (ne z cursor.idx — ten může být po
		// aktualizaci dat zastaralý).
		const rect = u.over.getBoundingClientRect();
		const t = u.posToVal(e.clientX - rect.left, 'x');
		const i = nearestIdx(t);
		if (i != null && ts[i] != null) {
			onpin(ts[i]);
		}
	}

	// Dvojklik přiblíží na místo, kam uživatel klikl: rozsah na polovinu
	// a okno posunuté tak, aby ten okamžik zůstal pod kurzorem. Bez
	// druhé části by se zoom táhl k pravému okraji a hledané místo by
	// z obrazu uteklo.
	function onDblClick(e) {
		if (!u) return;
		// Na šipce incidentu dvojklik nezoomuje — první klik už odešel
		// na stránku incidentu.
		if (markerAt(e.clientX, e.clientY)) return;
		e.preventDefault();
		const last = lastTs();
		if (last == null) return;
		const rect = u.over.getBoundingClientRect();
		const frac = Math.min(1, Math.max(0, (e.clientX - rect.left) / Math.max(rect.width, 1)));
		const at = u.posToVal(e.clientX - rect.left, 'x');
		const next = Math.round(Math.max(SPAN_MIN, span / 2));
		if (next === span) return;
		span = next;
		// Konec okna = bod pod kurzorem + kolik z nového rozsahu má být
		// napravo od něj.
		setEnd(at + next * (1 - frac));
		applyScale();
	}

	// Tažení spodní čáry (pan indikátoru) — grab & scroll historií.
	let trackEl;
	let trackDrag = false;

	function trackSeek(clientX) {
		const last = lastTs();
		if (!trackEl || last == null) return;
		const rect = trackEl.getBoundingClientRect();
		const frac = Math.min(1, Math.max(0, (clientX - rect.left) / rect.width));
		// frac = střed okna v hodinové historii
		const histStart = last - SPAN_MAX;
		setEnd(histStart + frac * SPAN_MAX + span / 2);
		scheduleFlush();
	}

	function onTrackDown(e) {
		trackDrag = true;
		trackSeek(e.clientX);
	}
	function onWinMove(e) {
		if (trackDrag) trackSeek(e.clientX);
	}
	function onWinUp() {
		trackDrag = false;
		dragging = false;
	}

	function build() {
		const faint = cssVar('--text-faint') || '#5c5c63';
		const grid = 'rgba(255,255,255,0.07)';
		const netDown = cssVar('--net-down') || '#7cc0ff';
		const netUp = cssVar('--net-up') || '#c4a7ff';

		u?.destroy();
		const series = isNet
			? [
					{},
					{ width: 1.6, stroke: netDown, fill: rgba(hexToRgb(netDown), 0.06) },
					{ width: 1.6, stroke: netUp, fill: rgba(hexToRgb(netUp), 0.06) }
				]
			: [
					{},
					{
						width: 1.8,
						stroke: (uu) => loadGradient(uu, 1),
						fill: (uu) => loadGradient(uu, 0.07)
					}
				];

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
					// Žádné tečky při hoveru — jen linka; tečka patří zámku.
					points: { show: false }
				},
				scales: {
					y: isNet
						? { range: (_u, _min, max) => [0, Math.max((max ?? 0) * 1.15, 1024 * 1024)] }
						: { range: [0, 100] }
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
						size: isNet ? 64 : 42,
						ticks: { show: false },
						grid: { stroke: grid, width: 1, dash: [2, 4] },
						values: (_u, vals) => vals.map((v) => (isNet ? fmtBps(v) : `${v} %`))
					}
				],
				series,
				hooks: {
					setCursor: [
						(uu) => {
							const i = uu.cursor.idx;
							if (i == null || ts[i] == null) {
								onhover(null);
							} else {
								onhover({ t: ts[i], i });
							}
						}
					],
					draw: [
						(uu) => {
							drawGaps(uu);
							drawMarkers(uu);
							positionPin();
						}
					]
				}
			},
			chartData(),
			el
		);

		pinLineEl = document.createElement('div');
		pinLineEl.className = 'pin-line';
		pinLineEl.style.display = 'none';
		pinDotEl = document.createElement('div');
		pinDotEl.className = 'pin-dot';
		pinDotEl.style.display = 'none';
		pinLabelEl = document.createElement('div');
		pinLabelEl.className = 'pin-label';
		pinLabelEl.style.display = 'none';
		u.over.appendChild(pinLineEl);
		u.over.appendChild(pinDotEl);
		u.over.appendChild(pinLabelEl);
		u.over.addEventListener('wheel', onWheel, { passive: false });
		u.over.addEventListener('click', onClick);
		u.over.addEventListener('dblclick', onDblClick);
		u.over.addEventListener('mousedown', onMouseDown);
		u.over.addEventListener('mousemove', onMouseMove);
		u.over.addEventListener('mouseup', onMouseUp);
		// auxclick = kliknutí kolečkem — nesmí se počítat jako zámek
		u.over.addEventListener('auxclick', (e) => e.preventDefault());
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

	// POZOR na reaktivitu: těla efektů běží v untrack(), aby efekt
	// závisel JEN na vyjmenovaných hodnotách. Bez toho si build()
	// přečte i ts/values a graf se přestaví při každém ticku dat —
	// zmizí kurzor, zámek i rozjetá interakce.
	$effect(() => {
		mode;
		untrack(() => {
			if (u && el) build();
		});
	});

	$effect(() => {
		// závislosti: jen data
		ts;
		values;
		values2;
		untrack(() => {
			if (!u) return;
			u.setData(chartData(), false);
			applyScale();
			// Kurzor se sám přepočítává jen při pohybu myši — po posunu
			// dat pod nehybnou myší ho znovu vyhodnotíme, aby nezamrzl.
			if (u.cursor.left != null && u.cursor.left >= 0) {
				u.setCursor({ left: u.cursor.left, top: u.cursor.top });
			}
		});
	});

	$effect(() => {
		pinned;
		untrack(() => positionPin());
	});

	$effect(() => {
		// závislost: markery incidentů — jen překreslení canvasu
		markers;
		untrack(() => u?.redraw(false));
	});
</script>

<div class="wrap">
	<div bind:this={el} class="chart"></div>

	{#if isNet}
		<div class="legend label-tech">
			<span><i class="sw down"></i> {labels[0]}</span>
			<span><i class="sw up"></i> {labels[1]}</span>
		</div>
	{/if}

	<!-- Zoom indikátor: střed = odzoomováno, plná výška = přizoomováno -->
	<div
		class="zoom-ind"
		class:hidden={span === SPAN_DEFAULT}
		style:height={`${50 + 50 * zoomT}%`}
	></div>

	<!-- Pan indikátor: pozice okna v hodinové historii (skrytý na živě);
	     jde grabnout a scrollovat tažením -->
	<!-- svelte-ignore a11y_no_static_element_interactions -->
	<div
		class="pan-ind"
		class:hidden={follow && !trackDrag}
		bind:this={trackEl}
		onmousedown={onTrackDown}
	>
		<div
			class="thumb"
			style:left={`${panFrac.left * 100}%`}
			style:width={`${Math.max(panFrac.width * 100, 2)}%`}
		></div>
	</div>
</div>

<svelte:window onmousemove={onWinMove} onmouseup={onWinUp} />

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
	.chart :global(.pin-line) {
		position: absolute;
		top: 0;
		bottom: 0;
		width: 0;
		border-left: 1px solid rgba(255, 255, 255, 0.55);
		pointer-events: none;
	}
	/* Štítek zámku s časem — přilepený k lince. */
	.chart :global(.pin-label) {
		position: absolute;
		top: 4px;
		padding: 2px 6px;
		border: 1px solid var(--border-strong);
		border-radius: var(--radius-sm);
		background: rgba(20, 21, 26, 0.92);
		font-family: var(--font-mono);
		font-size: var(--fs-3xs);
		letter-spacing: 0.04em;
		color: var(--text);
		pointer-events: none;
		white-space: nowrap;
	}
	/* Tečka zámku — plná linka, dutý střed v barvě hodnoty. */
	.chart :global(.pin-dot) {
		position: absolute;
		width: 9px;
		height: 9px;
		border-radius: 50%;
		border: 1.6px solid var(--accent);
		background: transparent;
		transform: translate(-50%, -50%);
		pointer-events: none;
	}

	.legend {
		display: flex;
		gap: 1.2rem;
		margin-top: 0.45rem;
	}
	.legend .sw {
		display: inline-block;
		width: 14px;
		height: 2px;
		vertical-align: middle;
		margin-right: 0.35rem;
	}
	.legend .sw.down {
		background: var(--net-down);
	}
	.legend .sw.up {
		background: var(--net-up);
	}

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
		height: 5px;
		margin-top: 4px;
		border-radius: 2px;
		background: rgba(255, 255, 255, 0.06);
		transition: opacity 200ms ease-out;
		cursor: grab;
	}
	.pan-ind:active {
		cursor: grabbing;
	}
	.pan-ind .thumb {
		position: absolute;
		top: 0;
		bottom: 0;
		border-radius: 2px;
		background: rgba(255, 255, 255, 0.28);
		pointer-events: none;
	}
	.hidden {
		opacity: 0;
		pointer-events: none;
	}
</style>
