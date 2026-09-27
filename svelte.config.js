/** @type {import("@sveltejs/vite-plugin-svelte").SvelteConfig} */
export default {
  compilerOptions: {
    warningFilter: (warning) => !warning.code.startsWith("a11y"),
  },
};
