export type AudioSettings = {noise:'off'|'browser'|'rnnoise';echo:boolean;autoGain:boolean;gain:number;volume:number};
export const defaultAudioSettings:AudioSettings={noise:'browser',echo:true,autoGain:true,gain:1,volume:1};
export function processingLabel(noise:unknown,actual:unknown):string {
  if(noise==='listen')return '仅收听';
  if(noise==='rnnoise')return 'RNNoise 增强降噪';
  if(noise!=='browser')return '降噪已关闭';
  const enabled=actual&&typeof actual==='object'&&'noiseSuppression' in actual?actual.noiseSuppression:undefined;
  return enabled===true?'浏览器原生降噪':enabled===false?'原生降噪 · 设备未启用':'原生降噪 · 设备未报告状态';
}
export function readAudioSettings():AudioSettings {
  try {
    const s=JSON.parse(localStorage.getItem('webts-audio')??'{}');
    return {noise:['off','browser','rnnoise'].includes(s.noise)?s.noise:'browser',echo:typeof s.echo==='boolean'?s.echo:true,autoGain:typeof s.autoGain==='boolean'?s.autoGain:true,gain:Number.isFinite(s.gain)?Math.max(0,Math.min(2,s.gain)):1,volume:Number.isFinite(s.volume)?Math.max(0,Math.min(1,s.volume)):1};
  } catch {return {...defaultAudioSettings};}
}
