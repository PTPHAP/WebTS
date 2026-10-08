export type AudioSettings = {noise:'off'|'rnnoise';keyboard:boolean;echo:boolean;autoGain:boolean;gain:number;volume:number};
export const defaultAudioSettings:AudioSettings={noise:'rnnoise',keyboard:true,echo:true,autoGain:true,gain:1,volume:1};
export function processingLabel(noise:unknown,actual:unknown):string {
  if(noise==='listen')return '仅收听';
  const settings=actual&&typeof actual==='object'?actual as MediaTrackSettings:{};
  const state=(value:unknown)=>value===true?'已启用':value===false?'未启用':'设备未报告';
  const label=noise==='keyboard'?'本地降噪 + 键盘声增强':noise==='rnnoise'?'本地 RNNoise 降噪':'降噪已关闭';
  return `${label} · 回声消除：${state(settings.echoCancellation)} · 自动增益：${state(settings.autoGainControl)}${settings.noiseSuppression===true?' · 设备仍开启原生降噪':''}`;
}
export function readAudioSettings():AudioSettings {
  try {
    const s=JSON.parse(localStorage.getItem('webts-audio')??'{}');
    return {noise:s.noise==='off'?'off':'rnnoise',keyboard:typeof s.keyboard==='boolean'?s.keyboard:true,echo:typeof s.echo==='boolean'?s.echo:true,autoGain:typeof s.autoGain==='boolean'?s.autoGain:true,gain:Number.isFinite(s.gain)?Math.max(0,Math.min(2,s.gain)):1,volume:Number.isFinite(s.volume)?Math.max(0,Math.min(1,s.volume)):1};
  } catch {return {...defaultAudioSettings};}
}
