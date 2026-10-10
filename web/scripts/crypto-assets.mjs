import {access} from 'node:fs/promises';
for (const name of ['webts_crypto.js','webts_crypto_bg.wasm']) {
  try { await access(new URL(`../public/crypto/${name}`,import.meta.url)); }
  catch { throw new Error('Missing Olm WASM assets. Run scripts/build-crypto.sh or scripts/build-crypto.ps1 from the project root before building/testing.'); }
}
