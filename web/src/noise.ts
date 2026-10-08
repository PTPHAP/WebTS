type Worklet=AudioWorkletNode & {destroy():void};
export type NoiseProcessor={input:AudioNode;output:AudioNode;mode:'rnnoise'|'keyboard';warning?:string;onchange?:(message:string)=>void;onerror?:()=>void;speaking?:boolean;onspeech?:(enabled:boolean)=>void;typing():void;destroy():void};

async function ready(node:Worklet) {
  try {
    await new Promise<void>((resolve,reject)=>{
      const timer=window.setTimeout(()=>reject(new Error('降噪初始化超时')),5000);
      node.port.onmessage=e=>{window.clearTimeout(timer);e.data.type==='ready'?resolve():reject(new Error('降噪初始化失败'));};
      node.onprocessorerror=()=>{window.clearTimeout(timer);reject(new Error('降噪处理器错误'));};
    });
    node.port.onmessage=null;node.onprocessorerror=null;
    return node;
  } catch(error){node.destroy();node.disconnect();throw error;}
}
async function module(context:AudioContext,model:string) {
  let timer:number|undefined;
  try {await Promise.race([context.audioWorklet.addModule(`/audio/${model}-0.4.1-${model==='rnnoise'?'voice3':'strength1'}.js`),new Promise((_,reject)=>{timer=window.setTimeout(()=>reject(new Error('降噪组件加载超时')),5000);})]);}
  finally {window.clearTimeout(timer);}
}
export async function createNoise(context:AudioContext,keyboard=false,voiceOnly=false,strength=1):Promise<NoiseProcessor> {
  strength=Math.max(0,Math.min(1,Number.isFinite(strength)?strength:1));
  if(context.sampleRate!==48000)throw new Error('本地降噪需要 48 kHz 音频');
  const {loadRnnoise}=await import('@sapphi-red/web-noise-suppressor');
  const wasmBinary=await loadRnnoise({url:'/audio/0.4.1-rnnoise.wasm',simdUrl:'/audio/0.4.1-rnnoise_simd.wasm'},{signal:AbortSignal.timeout(5000)});
  await module(context,'rnnoise');
  const rnnode=new AudioWorkletNode(context,'@sapphi-red/web-noise-suppressor/rnnoise',{processorOptions:{maxChannels:1,wasmBinary,voiceOnly,strength}});
  const rnnoise=await ready(Object.assign(rnnode,{destroy:()=>rnnode.port.postMessage('destroy')}));
  let failed=false;rnnoise.onprocessorerror=()=>{failed=true;};
  let gtcrn:Worklet|undefined,warning:string|undefined;
  try {
    if(keyboard)try {
      const response=await fetch('/audio/0.4.1-gtcrn.wasm',{signal:AbortSignal.timeout(5000)});
      if(!response.ok)throw new Error('键盘降噪模型加载失败');
      const binary=await response.arrayBuffer();
      await module(context,'gtcrn');
      const node=new AudioWorkletNode(context,'@sapphi-red/web-noise-suppressor/gtcrn',{processorOptions:{maxChannels:1,wasmBinary:binary,strength}});
      gtcrn=await ready(Object.assign(node,{destroy:()=>node.port.postMessage('destroy')}));
    } catch {warning='键盘声增强不可用，继续使用本地 RNNoise 降噪。';}
    if(context.state==='closed'||failed)throw new Error('语音连接已关闭或降噪处理器停止');
    const input=context.createGain();
    input.connect(gtcrn??rnnoise);gtcrn?.connect(rnnoise);if(gtcrn)rnnoise.port.postMessage({type:'preserve',enabled:true});
    const processor:NoiseProcessor={input,output:rnnoise,mode:gtcrn?'keyboard':'rnnoise',warning,speaking:false,typing(){rnnoise.port.postMessage({type:'typing'});},destroy(){input.disconnect();for(const node of [gtcrn,rnnoise])if(node){node.onprocessorerror=null;node.port.onmessage=null;node.destroy();node.disconnect();}gtcrn=undefined;}};
    rnnoise.port.onmessage=e=>{if(e.data.type==='speech'&&typeof e.data.enabled==='boolean'){processor.speaking=e.data.enabled;processor.onspeech?.(e.data.enabled);}};
    rnnoise.onprocessorerror=()=>processor.onerror?.();
    if(gtcrn)gtcrn.onprocessorerror=()=>{input.disconnect();gtcrn?.destroy();gtcrn?.disconnect();gtcrn=undefined;rnnoise.port.postMessage({type:'preserve',enabled:false});input.connect(rnnoise);processor.mode='rnnoise';processor.onchange?.('键盘声增强已停止，继续使用本地 RNNoise 降噪。');};
    return processor;
  } catch(error){gtcrn?.destroy();gtcrn?.disconnect();rnnoise.destroy();rnnoise.disconnect();throw error;}
}
