<script>
	// Tichý placeholder prázdné sekce (DESIGN.md kap. 8).
	let { title, note } = $props();
</script>

<div class="ph">
	<span class="label-tech">// {title}</span>
	<p>{note}</p>
</div>

<style>
	.ph {
		height: 100%;
		display: flex;
		flex-direction: column;
		align-items: center;
		justify-content: center;
		gap: 0.6rem;
	}
	p {
		margin: 0;
		color: var(--text-faint);
		font-size: 0.88rem;
	}
</style>
