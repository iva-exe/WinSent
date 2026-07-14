import adapter from '@sveltejs/adapter-static';

/** @type {import('@sveltejs/kit').Config} */
export default {
	kit: {
		// SPA — žádné přenačítání (SPEC kap. 9.4); fallback drží klientský routing.
		adapter: adapter({ fallback: 'index.html' })
	}
};
