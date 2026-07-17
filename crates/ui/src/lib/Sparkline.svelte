<script>
	// Mini graf zátěže (canvas, bez knihovny) — pro jádra CPU v detail
	// sekci. Škála pevně 0–100 %. Barva čáry = stejný vertikální
	// gradient podle zátěže jako hlavní graf (zelená dole → červená
	// nahoře), takže výška bodu odpovídá jeho barvě.
	// `marker` = index bodu, na kterém se vykreslí svislá linka (zámek
	// času) — okno dat se předává tak, aby byl bod uprostřed.
	let { values = [], height = 26, marker = null } = $props();

	let canvas;

	function cssVar(name) {
		return getComputedStyle(document.documentElement).getPropertyValue(name).trim();
	}

	$effect(() => {
		// závislosti: values, marker
		values;
		marker;
		if (!canvas) return;
		const dpr = window.devicePixelRatio || 1;
		const w = canvas.clientWidth;
		const h = height;
		if (w === 0) return;
		canvas.width = w * dpr;
		canvas.height = h * dpr;
		const ctx = canvas.getContext('2d');
		ctx.scale(dpr, dpr);
		ctx.clearRect(0, 0, w, h);
		if (values.length < 2) return;

		// Gradient odspoda (0 %) nahoru (100 %) — zastávky jako hlavní graf.
		const g = ctx.createLinearGradient(0, h, 0, 0);
		g.addColorStop(0, cssVar('--ok') || '#4ade80');
		g.addColorStop(0.55, cssVar('--ok') || '#4ade80');
		g.addColorStop(0.75, cssVar('--warn') || '#f59e0b');
		g.addColorStop(0.9, cssVar('--danger') || '#ef4444');
		g.addColorStop(1, cssVar('--danger') || '#ef4444');

		ctx.beginPath();
		const n = values.length;
		for (let i = 0; i < n; i++) {
			const x = (i / (n - 1)) * w;
			const y = h - (Math.min(values[i] ?? 0, 100) / 100) * (h - 2) - 1;
			if (i === 0) ctx.moveTo(x, y);
			else ctx.lineTo(x, y);
		}
		ctx.strokeStyle = g;
		ctx.lineWidth = 1.4;
		ctx.stroke();

		// Linka zámku času.
		if (marker != null && marker >= 0 && marker < n) {
			const mx = (marker / (n - 1)) * w;
			ctx.beginPath();
			ctx.moveTo(mx, 0);
			ctx.lineTo(mx, h);
			ctx.strokeStyle = 'rgba(255, 255, 255, 0.55)';
			ctx.lineWidth = 1;
			ctx.stroke();
		}
	});
</script>

<canvas bind:this={canvas} style:height={`${height}px`}></canvas>

<style>
	canvas {
		width: 100%;
		display: block;
	}
</style>
