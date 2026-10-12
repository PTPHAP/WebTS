import {defineConfig,devices} from '@playwright/test';

export default defineConfig({
  testDir:'tests/browser',testMatch:'private-messaging.spec.mjs',
  outputDir:'../.cache/e2ee-browser-results',workers:1,retries:0,timeout:30000,
  reporter:[['list']],use:{baseURL:'http://127.0.0.1:8092',trace:'off',screenshot:'off',video:'off'},
  webServer:{command:'npx vite build --config tests/browser/vite.config.mjs && npx vite preview --config tests/browser/vite.config.mjs --host 127.0.0.1 --port 8092 --strictPort',url:'http://127.0.0.1:8092/tests/browser/private-messaging.html',reuseExistingServer:false},
  projects:[
    {name:'chromium',use:{...devices['Desktop Chrome']}},
    {name:'firefox',use:{...devices['Desktop Firefox']}},
    {name:'webkit',use:{...devices['Desktop Safari']}},
    {name:'mobile-webkit-emulation',use:{...devices['iPhone 13']}},
  ],
});
