<script>
	// Malý průběh do dlaždice (canvas, bez knihovny).
	//
	// Proč ne LiveChart z Tasks: ten má na kolečku vlastní obsluhu
	// s preventDefault, takže by rolování myší nad dlaždicí zastavilo
	// posun celého přehledu. Widget navíc žádný pan ani zoom nechce —
	// je to náhled, ne nástroj.
	//
	// Škála: procentní režim má pevných 0–100 (jinak by 3% špička
	// vypadala jako vytížený stroj), síťový režim se přizpůsobuje
	// maximu v okně, protože absolutní strop u linky neexistuje.
	let {
		/// Hodnoty zleva doprava; poslední je „teď". `null` = metrika,
		/// kterou stroj v tom vzorku nehlásil.
		values = [],
		/// Druhá série (upload) — kreslí se slabší barvou.
		values2 = null,
		/// 'pct' = pevná osa 0–100, 'auto' = podle maxima.
		skala = 'pct',
		barva = 'var(--ok)',
		barva2 = 'var(--net-up)',
		/// Výška v bodech; `null` = vyplnit, co dá rodič (dlaždice se
		/// dá natáhnout, takže pevná čísla by se rozešla).
		vyska = 44,
		/// Vyplnit plochu pod křivkou?
		vypln = true
	} = $props();

	let canvas;
	// Dlaždice se dá natáhnout za hranu a plátno se samo nepřekreslí —
	// do dalšího vzorku by zůstala roztažená stará bitmapa. Tohle je ta
	// jediná závislost, kterou nejde vyčíst z dat.
	let zmenaVelikosti = $state(0);

	$effect(() => {
		if (!canvas) return;
		const ro = new ResizeObserver(() => (zmenaVelikosti += 1));
		ro.observe(canvas);
		return () => ro.disconnect();
	});

	function cssBarva(v) {
		const s = String(v).trim();
		if (!s.startsWith('var(')) return s;
		const jmeno = s.slice(4, -1).trim();
		return getComputedStyle(document.documentElement).getPropertyValue(jmeno).trim() || '#888';
	}

	function krivka(ctx, data, w, h, max, barvaCss, plnit) {
		if (data.length < 2) return;
		const dx = w / (data.length - 1);
		ctx.beginPath();
		data.forEach((v, i) => {
			const y = h - Math.min((v ?? 0) / max, 1) * (h - 2) - 1;
			if (i === 0) ctx.moveTo(0, y);
			else ctx.lineTo(i * dx, y);
		});
		ctx.strokeStyle = barvaCss;
		ctx.lineWidth = 1.5;
		ctx.lineJoin = 'round';
		ctx.stroke();
		if (!plnit) return;
		ctx.lineTo(w, h);
		ctx.lineTo(0, h);
		ctx.closePath();
		const g = ctx.createLinearGradient(0, 0, 0, h);
		g.addColorStop(0, barvaCss + '38');
		g.addColorStop(1, barvaCss + '00');
		ctx.fillStyle = g;
		ctx.fill();
	}

	$effect(() => {
		// Závislosti: pole se mění na místě, takže se čte i délka
		// a poslední hodnota — jinak by se efekt po push nespustil.
		values.length;
		values[values.length - 1];
		values2?.length;
		skala;
		vyska;
		zmenaVelikosti;
		if (!canvas) return;
		const w = canvas.clientWidth;
		const h = vyska ?? canvas.clientHeight;
		if (!w || !h) return;
		const dpr = window.devicePixelRatio || 1;
		canvas.width = w * dpr;
		canvas.height = h * dpr;
		const ctx = canvas.getContext('2d');
		ctx.scale(dpr, dpr);
		ctx.clearRect(0, 0, w, h);

		let max = 100;
		if (skala === 'auto') {
			const vse = values2 ? [...values, ...values2] : values;
			max = Math.max(1, ...vse.map((v) => v ?? 0)) * 1.15;
		}
		if (values2) krivka(ctx, values2, w, h, max, cssBarva(barva2), false);
		krivka(ctx, values, w, h, max, cssBarva(barva), vypln);
	});
</script>

<canvas bind:this={canvas} style:height={vyska ? `${vyska}px` : '100%'}></canvas>

<style>
	canvas {
		display: block;
		width: 100%;
	}
</style>
