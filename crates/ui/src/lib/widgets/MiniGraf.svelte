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
		/// Hodnoty zleva doprava; poslední je „teď".
		values = [],
		/// Druhá série (upload) — kreslí se slabší barvou.
		values2 = null,
		/// 'pct' = pevná osa 0–100, 'auto' = podle maxima.
		skala = 'pct',
		barva = 'var(--ok)',
		barva2 = 'var(--net-up)',
		vyska = 44,
		/// Vyplnit plochu pod křivkou?
		vypln = true
	} = $props();

	let canvas;

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
			const y = h - Math.min(v / max, 1) * (h - 2) - 1;
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
		if (!canvas) return;
		const w = canvas.clientWidth;
		if (!w) return;
		const dpr = window.devicePixelRatio || 1;
		canvas.width = w * dpr;
		canvas.height = vyska * dpr;
		const ctx = canvas.getContext('2d');
		ctx.scale(dpr, dpr);
		ctx.clearRect(0, 0, w, vyska);

		let max = 100;
		if (skala === 'auto') {
			const vse = values2 ? [...values, ...values2] : values;
			max = Math.max(1, ...vse) * 1.15;
		}
		if (values2) krivka(ctx, values2, w, vyska, max, cssBarva(barva2), false);
		krivka(ctx, values, w, vyska, max, cssBarva(barva), vypln);
	});
</script>

<canvas bind:this={canvas} style:height="{vyska}px"></canvas>

<style>
	canvas {
		display: block;
		width: 100%;
	}
</style>
