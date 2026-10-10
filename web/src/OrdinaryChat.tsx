import {useEffect,useRef,useState} from 'react';
import {api} from './api';
import {useAccountPreference} from './account-preferences';
import {ImageCropper} from './ImageCropper';
import {Sticker,StickerPicker} from './Stickers';
import {burnDurations,openServerMessage,sealServerMessage} from './friend-crypto';
import type {Envelope} from './friend-crypto';

type Message={seq:number;id:string;sender:number;recipient:number;created_at:number;expires:number;burn:boolean;burn_seconds?:number|null;read_at:number|null;mode:'e2ee'|'server';epoch:number;value?:Envelope;error?:string};
export type OrdinaryPending={peer:number;id:string;ciphertext:string;burn:boolean;burn_seconds?:number;mode:'server';epoch:number};
// Ordinary chat needs only the authenticated account, never an Olm device lease.
// Decrypted content stays in this mounted conversation; it is not persisted.
export function OrdinaryChat({account,peer,name,epoch,active,enabled,days,refresh,pending,setPending}:{account:number;peer:number;name:string;epoch:number;active:boolean;enabled:boolean;days:number;refresh:()=>Promise<void>;pending:OrdinaryPending|undefined;setPending:(value:OrdinaryPending|undefined)=>void}){
  const[rows,setRows]=useState<Message[]>([]),[draft,setDraft]=useState(''),[image,setImage]=useState(''),[crop,setCrop]=useState<File>(),[burn,setBurn]=useAccountPreference('burn',false),[seconds,setSeconds]=useAccountPreference('burnSeconds',60),[status,setStatus]=useState(''),[busy,setBusy]=useState(false);
  const visible=useRef(active);visible.current=active;
  const alive=useRef(false),working=useRef(false),sending=useRef(false),current=useRef(rows),log=useRef<HTMLDivElement>(null);current.current=rows;
  async function receive(){if(working.current||sending.current||!alive.current||!visible.current||document.visibilityState!=='visible')return;working.current=true;try{
    const data=await api<{messages:Message[]}>(`/friends/${peer}/messages`);const result:Message[]=[];
    for(const row of [...data.messages].reverse()){
      if(!alive.current||!visible.current)return;if(row.mode!=='server')continue;
      const cached=current.current.find(r=>r.id===row.id&&r.epoch===row.epoch&&r.value);const next={...row,value:cached?.value,error:undefined as string|undefined};
      try{if(!next.value){const packet=await api<{ciphertext:string}>(`/friends/messages/${row.id}`);if(!alive.current)return;next.value=await openServerMessage(packet.ciphertext,row.epoch,{site:location.origin,id:row.id,sender:row.sender,recipient:row.recipient,burn:row.burn,burn_seconds:row.burn_seconds??undefined});}
        if(!alive.current||!visible.current)return;next.expires=Math.min(next.expires,next.value.expires,cached?.expires??Infinity);
        if(row.recipient===account&&row.read_at===null){if(row.burn)next.expires=Math.min(next.expires,Date.now()/1000+(row.burn_seconds??60));try{const ack=await api<{expires:number}>(`/friends/messages/${row.id}/read`,{});next.expires=Math.min(next.expires,ack.expires);}catch{setStatus('已读确认等待重试，焚毁计时不会延长。');}}
        if(next.expires>Date.now()/1000)result.push(next);
      }catch(e){next.error=e instanceof Error?e.message:'读取失败';result.push(next);}
    }
    // Bound decoded memory, including pictures, independently of bucket quotas.
    let bytes=0;const bounded:Message[]=[];for(const row of result.reverse()){bytes+=JSON.stringify(row.value??{}).length*2;if(bytes>2*1024*1024)break;bounded.unshift(row);}if(alive.current)setRows(bounded);
  }catch(e){if(alive.current)setStatus(e instanceof Error?e.message:'读取失败');}finally{working.current=false;}}
  useEffect(()=>{alive.current=true;void receive();const poll=setInterval(()=>{if(document.visibilityState==='visible')void receive();},5000);const expire=setInterval(()=>setRows(v=>{const next=v.filter(r=>r.expires>Date.now()/1000);return next.length===v.length?v:next;}),1000);return()=>{alive.current=false;clearInterval(poll);clearInterval(expire);};},[]);
  useEffect(()=>{if(active)void receive();},[active]);
  useEffect(()=>{if(log.current)log.current.scrollTop=log.current.scrollHeight;},[rows.length]);
  useEffect(()=>{const warn=(e:BeforeUnloadEvent)=>{if(pending){e.preventDefault();}};window.addEventListener('beforeunload',warn);return()=>{window.removeEventListener('beforeunload',warn);};},[pending]);
  async function send(sticker?:string){if(sending.current||busy||!enabled)return;if(!pending&&!draft.trim()&&!image&&!sticker)return;sending.current=true;setBusy(true);let outgoing=pending;
    try{if(!outgoing){const value:Envelope={version:1,site:location.origin,id:crypto.randomUUID(),sender:account,recipient:peer,expires:Math.floor(Date.now()/1000)+days*86400,text:draft,image:image||undefined,sticker,burn,burn_seconds:burn?seconds:undefined};outgoing={peer,id:value.id,ciphertext:await sealServerMessage(value,epoch),burn,burn_seconds:value.burn_seconds,mode:'server',epoch};if(!alive.current)return;setPending(outgoing);}
      await api('/friends/messages',outgoing);if(alive.current){setPending(undefined);setDraft('');setImage('');setStatus('已加密保存至对象存储 · 对方上线可读取');void refresh().catch(()=>{});}
    }catch(e){if(alive.current)setStatus(`${e instanceof Error?e.message:'发送失败'}；可重试同一份密文。`);}finally{sending.current=false;if(alive.current){setBusy(false);void receive();}}
  }
  return <><p className="friend-status">普通聊天 · HTTPS 传输与 AES-256-GCM 加密存储。本站可以解密；只有双方账号可通过接口读取，不需要私信身份、口令或对方确认。消息加密保存至对象存储，不写入本地聊天记录。</p>{!enabled&&<p role="alert">管理员尚未启用私有对象存储，暂不能发送；不会降级为明文保存。</p>}{status&&<p role="status" className="friend-status">{status}</p>}
    <div className="friend-messages" ref={log} role="log" aria-label="普通好友私信">{rows.length===0&&<div className="chat-empty"><p>向 {name} 发送第一条消息</p><small>临时消息最多保留 {days} 天 · 已读后可定时焚毁</small></div>}{rows.map(row=><article className={`friend-message ${row.sender===account?'self':''}`} key={row.id}><small>{row.sender===account?'你':name} · {new Date(row.created_at*1000).toLocaleTimeString([],{hour:'2-digit',minute:'2-digit'})} · 本站可解密</small>{row.value?<><p>{row.value.text}</p>{row.value.image&&<img src={row.value.image} alt="聊天图片" loading="lazy"/>}{row.value.sticker&&<Sticker id={row.value.sticker}/>}</>:<p>{row.error}</p>}{row.burn&&<small>已读后 {burnDurations.find(([v])=>v===row.burn_seconds)?.[1]??'60秒'}焚毁，服务器保留上限优先。仍可截图或保存。</small>}</article>)}</div>
    {image&&<div className="friend-attachment"><img src={image} alt="待发图片"/><button onClick={()=>setImage('')}>移除</button></div>}
    <div className="friend-compose-options"><label className="checkbox"><input type="checkbox" checked={burn} disabled={!!pending} onChange={e=>setBurn(e.target.checked)}/>阅后即焚</label>{burn&&<label className="friend-burn-duration">已读后<select aria-label="阅后即焚时长" value={seconds} disabled={!!pending} onChange={e=>setSeconds(Number(e.target.value))}>{burnDurations.map(([v,label])=><option key={v} value={v}>{label}</option>)}</select></label>}<label className="text-button file-button">图片 / 自定义表情<input type="file" accept="image/png,image/jpeg,image/webp" disabled={busy||!!pending} onChange={e=>{const file=e.target.files?.[0];e.target.value='';if(file)setCrop(file);}}/></label>{pending&&<><button className="text-button" disabled={busy} onClick={()=>void send()}>重试待发消息</button><button className="text-button danger" disabled={busy} onClick={()=>{if(window.confirm('放弃本页待发密文？服务器若已收到仍可能送达。'))setPending(undefined);}}>放弃待发</button></>}</div>
    <form className="composer friend-composer" onSubmit={e=>{e.preventDefault();void send();}}><StickerPicker privacy="server" emoji={t=>setDraft(v=>(v+t).slice(0,4096))} sticker={id=>{if(!pending)void send(id);}}/><input aria-label="普通好友私信内容" value={draft} onChange={e=>setDraft(e.target.value)} maxLength={4096} disabled={busy||!!pending} placeholder="发送普通私信（本站可解密）…"/><button aria-label="发送普通私信" disabled={busy||!enabled||!pending&&!draft.trim()&&!image}>➜</button></form>{crop&&<ImageCropper file={crop} purpose="content" onCancel={()=>setCrop(undefined)} onComplete={value=>{setImage(value);setCrop(undefined);}}/>}
  </>;
}
