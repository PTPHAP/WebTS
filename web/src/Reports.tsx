import {useCallback,useEffect,useRef,useState} from 'react';
import type {FormEvent} from 'react';
import {api} from './api';
import './reports.css';
export type ReportTarget={kind:'account'|'event';target:string;label:string};
type Report={id:string;kind:string;target:string;reason:string;reply:string;note?:string;owner?:number;status:string;revision:number;created_at:number;updated_at:number;audit?:{actor:number;status:string;created_at:number}[]};
type Reply={account:number;items:Report[];next:number|null};
const status:Record<string,string>={pending:'待处理',processing:'处理中',resolved:'已处理',dismissed:'不予处理'};
const date=(n:number)=>new Date(n*1000).toLocaleString('zh-CN');
export function ReportCenter({account,admin=false,target}:{account:number;admin?:boolean;target?:ReportTarget}){
 const[rows,setRows]=useState<Report[]>([]),[targets,setTargets]=useState<ReportTarget[]>(target?[target]:[]),[selected,setSelected]=useState(target?`${target.kind}:${target.target}`:''),[reason,setReason]=useState(''),[error,setError]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState(false),[ready,setReady]=useState(false),[before,setBefore]=useState<number|undefined>(),[next,setNext]=useState<number|null>(null),[pages,setPages]=useState<(number|undefined)[]>([]);
 const alive=useRef(false),working=useRef(false),query=useRef<AbortController|undefined>(undefined),request=useRef(crypto.randomUUID());
 const path=admin?'/admin/reports':'/reports';
 const load=useCallback(async()=>{
  try{
   const reply=await api<Reply>(`${path}?account=${account}${before?`&before=${before}`:''}`,undefined,undefined,query.current?.signal);
   if(!alive.current)return;if(reply.account!==account||!Array.isArray(reply.items))throw Error('账号已变化，请重新打开举报中心');setRows(reply.items);setNext(reply.next);setReady(true);
  }catch(e){if(alive.current){setRows([]);setReady(false);setNext(null);}throw e;}
 },[account,path,before]);
 const run=useCallback(async(action:()=>Promise<void>)=>{
  if(working.current)return;working.current=true;setBusy(true);setError('');setMessage('');const controller=new AbortController();query.current=controller;const timer=setTimeout(()=>controller.abort(),10000);
  try{await action();}catch(e){if(alive.current)setError(e instanceof Error?e.message:'操作失败，请重试');}finally{clearTimeout(timer);if(query.current===controller)query.current=undefined;working.current=false;if(alive.current)setBusy(false);}
 },[]);
 useEffect(()=>{alive.current=true;void run(load);return()=>{alive.current=false;query.current?.abort();};},[load,run]);
 useEffect(()=>{if(admin)return;let active=true;const controller=new AbortController(),timer=setTimeout(()=>controller.abort(),10000);
  void api<{account:number;friends:{id:number;name?:string}[]}>('/friends',undefined,undefined,controller.signal).then(reply=>{
   if(!active)return;const found:ReportTarget[]=target?[target]:[];
   if(reply.account===account)for(const f of reply.friends)found.push({kind:'account',target:String(f.id),label:f.name||`社区用户 ${f.id}`});
   setTargets([...new Map(found.map(t=>[`${t.kind}:${t.target}`,t])).values()]);
  }).catch(()=>{if(active)setTargets(target?[target]:[]);}).finally(()=>clearTimeout(timer));return()=>{active=false;clearTimeout(timer);controller.abort();};
 },[account,admin,target]);
 async function submit(e:FormEvent){e.preventDefault();const chosen=targets.find(t=>`${t.kind}:${t.target}`===selected);if(!chosen||!reason.trim())return;await run(async()=>{const reply=await api<{account:number;id:string}>('/reports',{account,kind:chosen.kind,target:chosen.target,reason,request_id:request.current},undefined,query.current?.signal);if(!alive.current)return;if(reply.account!==account)throw Error('账号已变化');setReason('');request.current=crypto.randomUUID();setMessage('举报已提交，处理结果可在下方查看。');await load();});}
 async function decide(row:Report,decision:{status:string;reply:string;note:string;request_id:string}){await run(async()=>{const reply=await api<{account:number}>(`/admin/reports/${row.id}`,{account,revision:row.revision,...decision},undefined,query.current?.signal);if(!alive.current)return;if(reply.account!==account)throw Error('账号已变化');setMessage('处理已保存。');await load();});}
 return <section className="report-center"><p className="muted">{admin?'举报不会自动封禁账号，也不会更改 TeamSpeak 权限。需要限制账号时，请在账号管理中单独操作。':'请说明发生了什么；不要粘贴密码、私钥或他人无关隐私。本站管理员可阅读你提交的内容，系统不会自动获取聊天记录。'}</p>{error&&<p className="error" role="alert">{error} <button className="text-button" disabled={busy} onClick={()=>void run(load)}>重新读取</button></p>}{message&&<p role="status">{message}</p>}
  {!admin&&<form className="report-form" onSubmit={submit}><label>举报对象<select required value={selected} disabled={busy} onChange={e=>{setSelected(e.target.value);request.current=crypto.randomUUID();}}><option value="">选择好友或好友请求</option>{targets.map(t=><option key={`${t.kind}:${t.target}`} value={`${t.kind}:${t.target}`}>{t.kind==='event'?'活动':'账号'} · {t.label}</option>)}</select></label><label>原因与必要说明<textarea required rows={4} maxLength={4000} value={reason} disabled={busy} onChange={e=>{setReason(e.target.value);request.current=crypto.randomUUID();}}/></label><small>文字最多4000字节；每天最多5条、未结案最多16条。材料加密保存在本站；结案后保留90天，所有举报最长180天。</small><button className="primary" disabled={busy||!selected||!reason.trim()}>提交举报</button></form>}
  <div className="report-toolbar"><h3>{admin?'举报处理':'我的举报'}</h3><button className="secondary small" disabled={busy} onClick={()=>void run(load)}>刷新</button></div>{!ready?<p role="status">{error?'读取失败':'正在读取…'}</p>:!rows.length?<p className="muted">暂无举报记录。</p>:<div className="report-list">{rows.map(row=><details className="report-case" key={`${row.id}:${row.revision}`}><summary><strong>{row.kind==='event'?'活动举报':'账号举报'}</strong><span>{status[row.status]??'未知状态'}</span><time>{date(row.updated_at)}</time></summary><p className="report-reference">对象编号：{row.target}{admin?` · 举报人账号编号：${row.owner}`:''}</p><p className="report-evidence">{row.reason}</p>{row.reply&&<blockquote>处理说明：{row.reply}</blockquote>}{admin&&<><DecisionForm report={row} busy={busy} save={v=>decide(row,v)}/><details><summary>操作记录（最近32条）</summary>{row.audit?.map((item,i)=><p key={i}>{date(item.created_at)} · 管理员 {item.actor} · {status[item.status]??item.status}</p>)}</details></>}</details>)}</div>}
  <div className="split-actions"><button className="secondary" disabled={busy||!pages.length} onClick={()=>{setBefore(pages.at(-1));setPages(pages.slice(0,-1));}}>上一页</button><button className="secondary" disabled={busy||!next} onClick={()=>{setPages([...pages,before]);setBefore(next!);}}>下一页</button></div>
 </section>;
}
function DecisionForm({report,busy,save}:{report:Report;busy:boolean;save:(value:{status:string;reply:string;note:string;request_id:string})=>Promise<void>}){
 const[value,setValue]=useState({status:report.status==='pending'?'processing':report.status,reply:report.reply,note:report.note??''}),request=useRef(crypto.randomUUID());
 const change=(patch:Partial<typeof value>)=>{setValue({...value,...patch});request.current=crypto.randomUUID();};
 return <form className="report-form" onSubmit={e=>{e.preventDefault();void save({...value,request_id:request.current});}}><label>处理状态<select disabled={busy} value={value.status} onChange={e=>change({status:e.target.value})}><option value="processing">处理中</option><option value="resolved">已处理</option><option value="dismissed">不予处理</option></select></label><label>给举报人的处理说明<textarea rows={3} disabled={busy} required={value.status!=='processing'} maxLength={2000} value={value.reply} onChange={e=>change({reply:e.target.value})}/></label><label>内部备注 · 仅管理员可见<textarea rows={2} disabled={busy} maxLength={2000} value={value.note} onChange={e=>change({note:e.target.value})}/></label><button className="primary" disabled={busy}>保存处理结果</button></form>;
}
