import {defineConfig} from 'vite';

export default defineConfig({build:{outDir:'../.cache/e2ee-browser-dist',emptyOutDir:true,rollupOptions:{input:['index.html','tests/browser/private-messaging.html']}}});
