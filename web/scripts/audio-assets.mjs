import {readFile,writeFile,copyFile,mkdir} from 'node:fs/promises';
const packageRoot=new URL('../node_modules/@sapphi-red/web-noise-suppressor/dist/',import.meta.url);
const target=new URL('../public/audio/',import.meta.url);
await mkdir(target,{recursive:true});
for(const model of ['rnnoise','gtcrn']) {
  let source=await readFile(new URL(`${model}/workletProcessor.js`,packageRoot),'utf8');
  // Pinned 0.4.1 has no ready/error acknowledgement. Keep its DSP unchanged.
  const marker='this.destroyed&&this.destroy()';
  if(source.split(marker).length!==2)throw new Error(`${model} source changed; review ready acknowledgement patch`);
  source=source.replace(marker,`${marker},this.port.postMessage({type:'ready'})`);
  const end='})()}process';
  if(source.split(end).length!==2)throw new Error(`${model} initialization source changed`);
  source=source.replace(end,"})().catch(()=>this.port.postMessage({type:'error'}))}process");
  source=source.replace(/\/\/# sourceMappingURL=.*$/m,'');
  await writeFile(new URL(`${model}-0.4.1-ready1.js`,target),source);
}
for(const file of ['rnnoise.wasm','rnnoise_simd.wasm','gtcrn.wasm'])await copyFile(new URL(file,packageRoot),new URL(`0.4.1-${file}`,target));
