import type {ConnectionQuality} from './connection-quality';
import {bundlePath} from './client-build';
export type AudioRuntime={context:string;processing:string;receivers:number;players:number;pausedPlayers:number};
export type QueueQuality={windowSeconds?:number;received?:number;dropped?:number;mean?:number;p95Upper?:number};
export function queueQuality(value:Record<string,unknown>):QueueQuality {
 const number=(n:unknown)=>typeof n==='number'&&Number.isFinite(n)&&n>=0?n:undefined;
 return {windowSeconds:number(value.window_seconds),received:number(value.received),dropped:number(value.dropped),mean:number(value.mean_ms),p95Upper:number(value.p95_upper_ms)};
}
export type DiagnosticInput={connected:boolean;reconnecting:boolean;transport:string;capture:string;listenOnly:boolean;muted:boolean;deafened:boolean;away:boolean;canSpeak:boolean;playbackBlocked:boolean;otherSpeaking:boolean;quality:ConnectionQuality;queue?:QueueQuality;tsRtt?:number;cipher:string;gain:number;volume:number;echoRequested?:boolean;echoActual?:boolean;runtime?:AudioRuntime;nodeRoute?:string;speechControl?:string};
export function voiceDiagnosis(s:DiagnosticInput):{title:string;help:string}[] {
  const items:{title:string;help:string}[]=[];
  const add=(title:string,help:string)=>items.push({title,help});
  if(!s.connected){add(s.reconnecting?'正在自动重连':'尚未连接','连接服务器后再检查语音；诊断不会自动断开或改变设备。');return items;}
  if(s.transport!=='connected')add('浏览器语音链路尚未就绪','检查网络及网关 UDP / TURN 配置；网页信令连接成功不代表语音链路成功。');
  if(!s.cipher)add('等待语音加密确认','加密未确认时不要把连接标为安全可用。');
  if(!s.listenOnly&&s.capture!=='running')add('麦克风采集暂停或恢复中','检查设备与网站麦克风权限；可点击恢复音频，频道连接保持。');
  if(!s.listenOnly&&s.echoRequested===true){
    if(s.echoActual===false)add('设备未启用回声消除','已请求开启，但设备报告未启用。检查网站麦克风权限与浏览器版本；外放时可先降低系统音量或使用耳机，避免回声传给对方。');
    else if(s.echoActual!==true)add('设备未报告回声消除状态','请求开启不代表实际生效；未报告也不代表已关闭。请双方分别做外放复测，不能用 AI 降噪代替回声消除。');
  }
  if(s.listenOnly)add('当前为仅收听连接','需要发言时重新选择带麦克风的连接。');
  else if(s.muted||s.away)add('麦克风主动暂停','取消静音或 AFK 后再发言。');
  else if(!s.canSpeak)add('当前身份没有发言权限','由 TeamSpeak 服务器管理员调整权限；网站不会额外授予权限。');
  else if(s.gain===0)add('麦克风增益为零','在语音设置中提高麦克风增益。');
  if(s.deafened||s.volume===0)add('收听已关闭或输出音量为零','恢复收听并检查系统音量及输出设备。');
  if(s.playbackBlocked)add('浏览器尚未允许播放','点击恢复音频；检查地址栏的网站声音权限。');
  if(s.otherSpeaking&&s.quality.received===0)add('有人发言，但尚未收到音频包','继续观察数秒；若持续如此，检查网关接收链路。仅凭这一快照不能确定丢包原因。');
  if(s.quality.statsAvailable===false)add('浏览器统计暂不可用','统计超时或浏览器不支持；通话连接保持，不使用旧数据判断当前质量。');
  if((s.quality.recentLoss??0)>5)add('近期接收丢包偏高','检查浏览器到网关链路及网络拥塞；近期数值按实际采样窗口计算。');
  else if(s.quality.recentLoss===undefined&&(s.quality.loss??0)>5)add('本次接收累计丢包偏高','检查两段链路；累计值不会立刻随网络恢复而下降。');
  if((s.quality.rtt??0)>150||(s.tsRtt??0)>150)add('网络往返时间偏高','优先排查对应链路和网络拥塞；RTT 不等于端到端语音延迟。');
  if((s.queue?.dropped??0)>0)add('网关已丢弃过期音频','网关转发队列等待超过200ms；检查网关资源与任务阻塞，避免积压旧声音。此项不包含入队前满队列丢弃。');
  if(!items.length)add('未发现明显的设置或连接异常','这是当前统计快照，不能代替真人听验或端到端延迟测量。');
  return items;
}
// A strict whitelist: never serialize RTCStatsReport, device IDs, names, addresses or chat.
export function diagnosticReport(s:DiagnosticInput,bundle?:string){
  const number=(v:unknown)=>typeof v==='number'&&Number.isFinite(v)&&v>=0?v:null;
  const enumeration=(v:string,allowed:string[])=>allowed.includes(v)?v:'unknown';
  const boolean=(v:unknown)=>typeof v==='boolean'?v:null;
  const count=(v:unknown)=>typeof v==='number'&&Number.isSafeInteger(v)&&v>=0?v:null;
  return {format:'webts-diagnostic-v5',voicePath:{gateway:enumeration(s.nodeRoute??'unknown',['site','node']),speechControl:enumeration(s.speechControl??'unknown',['direct','signalling','inactive'])},frontendBundle:bundlePath(bundle)??null,runtime:{context:enumeration(s.runtime?.context??'unknown',['running','suspended','interrupted','closed','inactive']),processing:enumeration(s.runtime?.processing??'unknown',['rnnoise','keyboard','off','blocked','listen','inactive']),receivers:count(s.runtime?.receivers),players:count(s.runtime?.players),pausedPlayers:count(s.runtime?.pausedPlayers)},audio:{echoRequested:boolean(s.echoRequested),echoActual:boolean(s.echoActual)},connection:{connected:s.connected===true,reconnecting:s.reconnecting===true,transport:enumeration(s.transport,['new','connecting','connected','disconnected','failed','closed']),capture:enumeration(s.capture,['running','paused','muted','ended','failed','recovering']),listenOnly:s.listenOnly===true,muted:s.muted===true,deafened:s.deafened===true,away:s.away===true,canSpeak:s.canSpeak===true,playbackBlocked:s.playbackBlocked===true},encryption:enumeration(s.cipher,['AES-256-GCM','AES-128-GCM']),queue:{windowSeconds:number(s.queue?.windowSeconds),receivedPackets:number(s.queue?.received),expiredPackets:number(s.queue?.dropped),meanMs:number(s.queue?.mean),p95UpperMs:number(s.queue?.p95Upper)},metrics:{statsAvailable:s.quality.statsAvailable===true,recentWindowSeconds:number(s.quality.windowSeconds),recentLossPercent:number(s.quality.recentLoss),recentBufferMs:number(s.quality.recentBuffer),recentConcealedPercent:number(s.quality.recentConcealed),recentConcealmentEvents:number(s.quality.recentConcealmentEvents),rttMs:number(s.quality.rtt),tsRttMs:number(s.tsRtt),jitterMs:number(s.quality.jitter),bufferMs:number(s.quality.buffer),lossPercent:number(s.quality.loss),receivedPackets:number(s.quality.received),sentPackets:number(s.quality.sent),route:enumeration(s.quality.route??'unknown',['direct','relay','unknown'])}};
}
