export type AudioSettings = {noise:'off'|'rnnoise';keyboard:boolean;voiceOnly:boolean;echo:boolean;autoGain:boolean;gain:number;volume:number;strength:number;receiveAutoGain:boolean;ducking:number;typing:boolean};
export const defaultAudioSettings:AudioSettings={noise:'rnnoise',keyboard:true,voiceOnly:true,echo:true,autoGain:true,gain:1,volume:1,strength:1,receiveAutoGain:true,ducking:.35,typing:true};
export function processingLabel(noise:unknown,actual:unknown,voiceOnly=false):string {
  if(noise==='listen')return '仅收听';
  if(noise==='blocked')return '人声识别不可用 · 麦克风发送已暂停，请重新连接';
  const settings=actual&&typeof actual==='object'?actual as MediaTrackSettings:{};
  const state=(value:unknown)=>value===true?'已启用':value===false?'未启用':'设备未报告';
  const label=noise==='keyboard'?'本地 AI 智能增强 · GTCRN':noise==='rnnoise'?'本地 AI 轻量保真 · RNNoise':'降噪已关闭';
  return `${label}${voiceOnly&&noise!=='off'?' + 仅保留人声':''} · 回声消除：${state(settings.echoCancellation)} · 自动增益：${state(settings.autoGainControl)}${settings.noiseSuppression===true?' · 设备仍开启原生降噪':''}`;
}
export function readAudioSettings():AudioSettings {
  try {
    const s=JSON.parse(localStorage.getItem('webts-audio')??'{}');
    return {noise:s.noise==='off'?'off':'rnnoise',keyboard:typeof s.keyboard==='boolean'?s.keyboard:true,voiceOnly:typeof s.voiceOnly==='boolean'?s.voiceOnly:true,echo:typeof s.echo==='boolean'?s.echo:true,autoGain:typeof s.autoGain==='boolean'?s.autoGain:true,gain:Number.isFinite(s.gain)?Math.max(0,Math.min(2,s.gain)):1,volume:Number.isFinite(s.volume)?Math.max(0,Math.min(1,s.volume)):1,strength:Number.isFinite(s.strength)?Math.max(0,Math.min(1,s.strength)):1,receiveAutoGain:typeof s.receiveAutoGain==='boolean'?s.receiveAutoGain:true,ducking:Number.isFinite(s.ducking)?Math.max(0,Math.min(1,s.ducking)):.35,typing:typeof s.typing==='boolean'?s.typing:true};
  } catch {return {...defaultAudioSettings};}
}
