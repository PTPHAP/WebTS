export type WatchStage={id:string;name:string;url:string;embed_enabled:boolean};
export type WatchConfig={enabled:boolean;title:string;description:string;url:string;cover:string;public:boolean;embed_enabled:boolean;ts_server:string;ts_channel:number;score_mode:'none'|'manual'|'webhook';stages?:WatchStage[]};
export type Score={tournament:string;stage:string;status:'scheduled'|'live'|'finished';best_of:number;teams:[{id:string;name:string;score:number;maps_won:number;side:string|null},{id:string;name:string;score:number;maps_won:number;side:string|null}];map:string;round:number;phase:string;bomb:string|null;players:{name:string;team:string;kills:number;deaths:number;assists:number;adr:number|null;health:number|null;armor:number|null;money:number|null;weapon:string|null}[];maps:{name:string;scores:[number,number]}[];rounds:string[];events:string[]};
export type WatchEvent={enabled:boolean;config?:WatchConfig;revision?:number;event_id?:string;platform?:string;feed_key_set?:boolean;server_time?:number;score?:{data:Score;updated_at:number;source:'manual'|'webhook'}|null};
export const emptyWatch:WatchConfig={enabled:false,title:'',description:'',url:'',cover:'',public:false,embed_enabled:false,ts_server:'',ts_channel:0,score_mode:'none'};
export const emptyScore:Score={tournament:'',stage:'',status:'scheduled',best_of:3,teams:[{id:'a',name:'队伍 A',score:0,maps_won:0,side:null},{id:'b',name:'队伍 B',score:0,maps_won:0,side:null}],map:'',round:0,phase:'',bomb:null,players:[],maps:[],rounds:[],events:[]};
export const platformNames:Record<string,string>={bilibili:'哔哩哔哩',huya:'虎牙',douyu:'斗鱼',youtube:'YouTube',twitch:'Twitch',link:'外部直播间'};
// Keep this narrow even if a stale/compromised config returns a different platform.
export function playerUrl(config:WatchConfig,hostname:string):string|null{
  if(!config.enabled||!config.embed_enabled)return null;
  let u:URL;try{u=new URL(config.url);}catch{return null;}
  if(u.protocol!=='https:'||u.username||u.password||u.port&&u.port!=='443')return null;
  const room=u.pathname.slice(1);
  if(u.hostname==='live.bilibili.com'&&/^\d{1,18}$/.test(room)){
    const id=Number(room);
    if(!Number.isSafeInteger(id)||id<=0)return null;
    return `https://www.bilibili.com/blackboard/live/live-activity-player.html?cid=${id}&quality=1&danmaku=0&mute=0`;
  }
  if(u.hostname==='www.youtube.com'&&u.pathname==='/watch'){
    const id=u.searchParams.get('v');if(id&&/^[\w-]{11}$/.test(id))return `https://www.youtube-nocookie.com/embed/${id}?playsinline=1&rel=0`;
  }
  if(u.hostname==='www.twitch.tv'&&/^[a-z\d_]{1,25}$/i.test(room)&&/^[a-z\d.-]+$/i.test(hostname))return `https://player.twitch.tv/?channel=${encodeURIComponent(room)}&parent=${encodeURIComponent(hostname)}&autoplay=false`;
  return null;
}
export function watchStages(config:WatchConfig):WatchStage[]{return [{id:'main',name:'主舞台',url:config.url,embed_enabled:config.embed_enabled},...(config.stages??[])];}
export function stageConfig(config:WatchConfig,id:string):WatchConfig{const stage=watchStages(config).find(s=>s.id===id)??watchStages(config)[0];return {...config,url:stage.url,embed_enabled:stage.embed_enabled};}
export function sourcePlatform(url:string):string{try{const h=new URL(url).hostname;return ({'live.bilibili.com':'bilibili','www.youtube.com':'youtube','www.twitch.tv':'twitch','www.huya.com':'huya','www.douyu.com':'douyu'} as Record<string,string>)[h]??'link';}catch{return 'link';}}
export function officialLink(url:string):string|null{try{const u=new URL(url);return u.protocol==='https:'&&!u.username&&!u.password?u.href:null;}catch{return null;}}
export function scoreFreshness(score:WatchEvent['score'],serverTime:number,elapsed:number){return !!score&&serverTime+elapsed-score.updated_at<=30;}
