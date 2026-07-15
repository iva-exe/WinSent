<script>
	// Mini graf zátěže (canvas, bez knihovny) — pro jádra CPU v detail
	// sekci. Škála pevně 0–100 %.
	let { values = [], color = '#ffffff', height = 26 } = $props();

	let canvas;

	$effect(() => {
		// závislosti: values, color
		values;
		color;
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
		ctx.beginPath();
		const n = values.length;
		for (let i = 0; i < n; i++) {
			const x = (i / (n - 1)) * w;
			const y = h - (Math.min(values[i] ?? 0, 100) / 100) * (h - 2) - 1;
			if (i === 0) ctx.moveTo(x, y);
			else ctx.lineTo(x, y);
		}
		ctx.strokeStyle = color;
		ctx.lineWidth = 1.4;
		ctx.stroke();
	});
</script>

<canvas bind:this={canvas} style:height={`${height}px`}></canvas>

<style>
	canvas {
		width: 100%;
		display: block;
	}
</style>
