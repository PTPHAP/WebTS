import {useCallback,useEffect,useRef,useState} from 'react';
import type {FormEvent} from 'react';
import {api} from './api';
import './account-security.css';

type LoginSession={id:string;created_at:number;browser:string;platform:string;remembered:boolean|null;expires:number;current:boolean};
type Reply={account:number;items:LoginSession[];total:number};
const date=(n:number)=>new Date(n*1000).toLocaleString('zh-CN');
const label=(s:LoginSession)=>s.browser?`${s.browser} · ${s.platform}`:'旧版登录 · 设备信息未记录';

export function AccountSecurity({account}:{account:number}){
  const[reply,setReply]=useState<Reply>(),[busy,setBusy]=useState(false),[error,setError]=useState(''),[message,setMessage]=useState(''),[target,setTarget]=useState<LoginSession|'others'>(),[password,setPassword]=useState('');
  const alive=useRef(false),working=useRef(false),query=useRef<AbortController|undefined>(undefined),passwordInput=useRef<HTMLInputElement>(null);
  const load=useCallback(async(signal:AbortSignal)=>{
    const next=await api<Reply>(`/auth/sessions?account=${account}`,undefined,undefined,signal);
    if(!alive.current||signal.aborted)return;
    if(next.account!==account||!Array.isArray(next.items))throw Error('账号已变化，请重新打开安全中心');
    setReply(next);setTarget(undefined);setPassword('');
  },[account]);
  const run=useCallback(async(action:(signal:AbortSignal)=>Promise<void>)=>{
    if(working.current)return;working.current=true;setBusy(true);setError('');setMessage('');
    const controller=new AbortController();query.current=controller;const timer=setTimeout(()=>controller.abort(),15000);
    try{await action(controller.signal);}catch(e){if(alive.current){setReply(undefined);setTarget(undefined);setPassword('');setError(e instanceof Error&&e.name!=='AbortError'?e.message:'请求超时，结果尚未确认，请刷新列表核对。');}}
    finally{clearTimeout(timer);if(query.current===controller)query.current=undefined;working.current=false;if(alive.current)setBusy(false);}
  },[]);
  useEffect(()=>{alive.current=true;void run(load);return()=>{alive.current=false;query.current?.abort();};},[load,run]);
  useEffect(()=>{if(!message)return;const timer=setTimeout(()=>setMessage(''),8000);return()=>clearTimeout(timer);},[message]);
  useEffect(()=>{if(target)passwordInput.current?.focus();},[target]);
  async function revoke(e:FormEvent){
    e.preventDefault();if(!reply||!target||!password||working.current)return;
    const chosen=target,secret=password;setPassword('');
    await run(async(signal)=>{
      const result=await api<{account:number;removed:number}>('/auth/sessions/revoke',{account,target:chosen==='others'?'others':chosen.id,password:secret},undefined,signal);
      if(!alive.current||signal.aborted)return;if(result.account!==account)throw Error('账号已变化，请重新打开安全中心');
      setTarget(undefined);await load(signal);if(alive.current&&!signal.aborted)setMessage(`已退出${result.removed}个登录会话，当前登录保留。`);
    });
  }
  return <section className="account-security" aria-label="账号安全中心">
    <div className="security-intro"><span className="eyebrow">ACCOUNT SECURITY</span><h3>掌握你的登录</h3><p>离开公共电脑后，可以在这里结束对应登录及其 TeamSpeak 连接。当前设备保持在线。</p></div>
    <div className="security-toolbar"><strong>{reply?`${reply.total} 个有效登录会话`:'登录会话'}</strong><button className="secondary small" disabled={busy} onClick={()=>void run(load)}>刷新列表</button></div>
    {error&&<p className="error" role="alert">{error}</p>}{message&&<p className="security-success" role="status">{message}</p>}
    {!reply?<p role="status">{busy?'正在核对登录状态…':'请刷新列表重新核对。'}</p>:<>
      <div className="security-session-list">{reply.items.map(s=><article className={`security-session ${s.current?'is-current':''}`} key={s.id}>
        <span className="security-device" aria-hidden="true">{s.platform.includes('Android')||s.platform.includes('iOS')?'▯':'▣'}</span>
        <div className="security-session-copy"><div className="security-session-title"><strong>{label(s)}</strong>{s.current&&<span className="security-current">当前登录</span>}</div><p>{s.created_at?`登录于 ${date(s.created_at)}`:'登录时间未记录'}</p><p>最晚失效：{date(s.expires)}</p><small>{s.remembered===null?'旧版登录未记录记住状态':s.remembered?'已选择记住登录 · 最长30天':'临时登录 · 最长12小时'}</small></div>
        {!s.current&&<button className="secondary danger small" disabled={busy} onClick={()=>{setPassword('');setTarget(s);setError('');setMessage('');}}>退出此会话</button>}
      </article>)}</div>
      {reply.total>reply.items.length&&<p className="muted">仅展示当前及最近50个会话；下方操作可退出全部其他会话。</p>}
      <button className="secondary danger security-others" disabled={busy||reply.total<=1} onClick={()=>{setPassword('');setTarget('others');setError('');setMessage('');}}>保留当前登录，退出其他会话</button>
    </>}
    {target&&reply&&<form className="security-confirm" onSubmit={revoke}><h4>{target==='others'?'退出全部其他登录？':`退出 ${label(target)}？`}</h4><p>这会终止对应会话的访问及语音连接，不会删除账号、TS身份或已保存的消息副本。请求中断时，请刷新核对实际结果。</p><label>验证当前网站密码<input ref={passwordInput} type="password" autoComplete="current-password" required maxLength={128} value={password} disabled={busy} onChange={e=>setPassword(e.target.value)}/></label><div className="split-actions"><button className="secondary" type="button" disabled={busy} onClick={()=>{setTarget(undefined);setPassword('');}}>取消</button><button className="primary" disabled={busy||!password}>{busy?'正在核对…':'确认退出'}</button></div></form>}
    <p className="security-footnote">一个浏览器可有多个登录，会话不等于物理设备。浏览器和系统名称来自可被伪造的粗略标识，仅供辨认，不作为身份凭据；本功能不保存IP、完整浏览器标识或设备指纹。列表不是实时在线状态，关闭浏览器也不一定会立即注销会话。</p>
  </section>;
}
