import {useEffect,useRef,useState} from 'react';
import {api} from './api';
type Data=Record<string,unknown>;
type Reply={account:number;initialized:boolean;preferences:Data};
const defaults:Data={},listeners=new Map<string,Set<(value:unknown)=>void>>();
let account:number|undefined,generation=0,ready=false,busy=false,cached:Data={},pending:Data={},sending:Data={},timer:ReturnType<typeof setTimeout>|undefined;
let controller:AbortController|undefined;
async function query(body?:unknown,method?:string){const run=new AbortController();controller=run;const timeout=setTimeout(()=>run.abort(),15000);run.signal.addEventListener('abort',()=>clearTimeout(timeout),{once:true});try{return await api<Reply>('/preferences',body,method,run.signal);}finally{clearTimeout(timeout);if(controller===run)controller=undefined;}}
let failed:(message:string)=>void=()=>{},reload:(()=>Promise<void>)|undefined;
function apply(data:Data){cached={...defaults,...data,...sending,...pending};for(const [name,callbacks] of listeners)for(const callback of callbacks)callback(cached[name]);}
function schedule(delay=300){clearTimeout(timer);timer=setTimeout(()=>{void (ready?flush():reload?.());},delay);}
async function flush(){if(!ready||busy||account===undefined||Object.keys(pending).length===0)return;const epoch=generation,owner=account;let retry=300;busy=true;sending=pending;pending={};try{const data=await query({account:owner,patch:sending},'PATCH');if(epoch!==generation)return;if(data.account!==owner)throw new Error('账号已变化，设置未同步');sending={};apply(data.preferences);}catch(e){if(epoch===generation){pending={...sending,...pending};sending={};failed(e instanceof Error?e.message:'设置同步失败');retry=5000;}}finally{if(epoch===generation){busy=false;if(Object.keys(pending).length)schedule(retry);}}}
export function startPreferences(owner:number,onError:(message:string)=>void,onReady:(ready:boolean)=>void=()=>{}):()=>void {
  controller?.abort();onReady(false);const epoch=++generation;account=owner;ready=false;busy=false;pending={};sending={};cached={};failed=onError;clearTimeout(timer);
  const refresh=async()=>{if(epoch!==generation||busy)return;busy=true;try{let data=await query();if(epoch!==generation)return;if(data.account!==owner)throw new Error('登录账号已变化');if(!data.initialized){data=await query({account:owner,patch:{...defaults,...pending},initialize:true},'PATCH');if(epoch!==generation)return;}if(data.account!==owner)throw new Error('登录账号已变化');ready=true;apply(data.preferences);onReady(true);}catch(e){if(epoch===generation){onReady(false);onError(e instanceof Error?e.message:'设置读取失败');if(!ready){clearTimeout(timer);timer=setTimeout(()=>{void refresh();},5000);}}}finally{if(epoch===generation){busy=false;if(ready&&Object.keys(pending).length)schedule();}}};
  apply({});reload=refresh;void refresh();const poll=setInterval(()=>{if(document.visibilityState==='visible')void refresh();},30000);window.addEventListener('focus',refresh);
  return()=>{if(epoch!==generation)return;generation++;controller?.abort();controller=undefined;reload=undefined;account=undefined;ready=false;busy=false;pending={};sending={};cached={};apply({});clearTimeout(timer);clearInterval(poll);window.removeEventListener('focus',refresh);};
}
export function useAccountPreference<T>(name:string,initial:T|(()=>T)):[T,(value:T|((before:T)=>T))=>void] {
  const[value,setValue]=useState<T>(()=>{const fallback=typeof initial==='function'?(initial as ()=>T)():initial;defaults[name]=fallback;return (cached[name]??fallback) as T;});const current=useRef(value);current.current=value;
  useEffect(()=>{const changed=(v:unknown)=>{current.current=v as T;setValue(v as T);};let callbacks=listeners.get(name);if(!callbacks){callbacks=new Set();listeners.set(name,callbacks);}callbacks.add(changed);if(cached[name]!==undefined)changed(cached[name]);return()=>{callbacks.delete(changed);};},[name]);
  return [value,next=>{const v=typeof next==='function'?(next as (before:T)=>T)(current.current):next;current.current=v;setValue(v);if(account===undefined)return;pending[name]=v;apply(cached);schedule();}];
}
