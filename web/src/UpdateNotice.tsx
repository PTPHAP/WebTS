import {useEffect,useRef,useState} from 'react';
import {readSeason} from './seasons';
import {bundlePath,loadedBundle} from './client-build';
export {bundlePath} from './client-build';
export function UpdateNotice({inCall,onSeason}:{inCall:boolean;onSeason?:(value:string)=>void}){
  const seasonReport=useRef(onSeason);seasonReport.current=onSeason;
  const[available,setAvailable]=useState(''),[dismissed,setDismissed]=useState('');
  useEffect(()=>{const current=loadedBundle(document,location.origin);if(!current)return;let alive=true,busy=false;let controller:AbortController|undefined;
    async function check(){if(busy||document.hidden)return;busy=true;controller=new AbortController();const timeout=setTimeout(()=>controller?.abort(),8000);try{const r=await fetch('/api/client-build',{cache:'no-store',credentials:'omit',signal:controller.signal});if(!r.ok)return;const data=await r.json();const value=bundlePath(data.bundle);if(alive&&Object.hasOwn(data,'season'))seasonReport.current?.(readSeason(data.season));if(alive&&value)setAvailable(value!==current?value:'');}catch{/* Offline or an older gateway is not an update failure. */}finally{clearTimeout(timeout);busy=false;}}
    const timer=setInterval(check,120000);const visible=()=>{if(!document.hidden)void check();};void check();document.addEventListener('visibilitychange',visible);return()=>{alive=false;controller?.abort();clearInterval(timer);document.removeEventListener('visibilitychange',visible);};},[]);
  if(!available||dismissed===available)return null;
  return <aside className="update-notice" role="status"><span><strong>新版界面已就绪</strong><small>{inCall?'正在通话，结束后再刷新。':'刷新即可使用新版；刷新会清除当前页面的未发送草稿。'}</small></span><button className="secondary small" disabled={inCall} onClick={()=>{if(confirm('刷新页面？未发送的草稿将丢失。'))location.reload();}}>刷新</button><button className="icon-button" aria-label="稍后更新" onClick={()=>setDismissed(available)}>×</button></aside>;
}
