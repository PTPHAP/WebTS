export async function createNoise(context:AudioContext) {
  const {RnnoiseWorkletNode,loadRnnoise}=await import('@sapphi-red/web-noise-suppressor');
  const wasmBinary=await loadRnnoise({url:'/audio/0.4.1-rnnoise.wasm',simdUrl:'/audio/0.4.1-rnnoise_simd.wasm'},{signal:AbortSignal.timeout(5000)});
  await Promise.race([context.audioWorklet.addModule('/audio/rnnoise-0.4.1-ready1.js'),new Promise((_,reject)=>window.setTimeout(()=>reject(new Error('降噪组件加载超时')),5000))]);
  const node=new RnnoiseWorkletNode(context,{maxChannels:1,wasmBinary});
  try {
    await new Promise<void>((resolve,reject)=>{
      const timer=window.setTimeout(()=>reject(new Error('降噪初始化超时')),5000);
      node.port.onmessage=e=>{window.clearTimeout(timer);e.data.type==='ready'?resolve():reject(new Error('降噪初始化失败'));};
      node.onprocessorerror=()=>{window.clearTimeout(timer);reject(new Error('降噪处理器错误'));};
    });
    return node;
  } catch(error){node.destroy();node.disconnect();throw error;}
}
