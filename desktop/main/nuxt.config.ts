// https://nuxt.com/docs/api/configuration/nuxt-config
export default defineNuxtConfig({
  compatibilityDate: "2024-04-03",

  postcss: {
    plugins: {
      tailwindcss: {},
      autoprefixer: {},
    },
  },

  css: ["~/assets/main.scss"],

  ssr: false,
  devtools: false,

  extends: [["../../libraries/base"]],

  app: {
    baseURL: "/main",
  },

  // /launch-picker is opened directly in its own window (not navigated to
  // from within the app), so the prerender crawler never discovers it via
  // links -- it has to be listed explicitly to get a static file at all.
  nitro: {
    prerender: {
      routes: ["/launch-picker"],
    },
  },
});
