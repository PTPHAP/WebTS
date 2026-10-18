import {useEffect,useRef,useState} from 'react';
import {api} from './api';

type Node={id:string;server:string;idle:number;enabled:boolean;use_gateway?:boolean;revision:number};
type Server={id:string;name:string;address:string};
type Code={code:string;node:string;expires:number;main_url:string};
export function AdminVoiceNodes({onSaved,refreshKey=0}:{onSaved?:()=>Promise<void>;refreshKey?:number}){
 const[nodes,setNodes]=useState<Node[]>([]),[servers,setServers]=useState<Server[]>([]),[password,setPassword]=useState(''),[code,setCode]=useState<Code|null>(null),[busy,setBusy]=useState(false),[message,setMessage]=useState('');
 const[choices,setChoices]=useState<Record<string,string>>({}),[ids,setIds]=useState<Record<string,string>>({});const alive=useRef(true),readRevision=useRef(0);
 async function load(signal?:AbortSignal){const revision=++readRevision.current;const v=await api<{nodes:Node[];servers?:Server[]}>('/admin/voice-nodes',undefined,undefined,signal);if(alive.current&&!signal?.aborted&&revision===readRevision.current){setNodes(v.nodes);setServers(v.servers??[]);}}
 useEffect(()=>{alive.current=true;let loading=false;const controller=new AbortController();const refresh=async()=>{if(loading)return;loading=true;try{await load(controller.signal);}catch{if(alive.current&&!controller.signal.aborted)setMessage('路线读取失败，请刷新后再操作。');}finally{loading=false;}};void refresh();const timer=window.setInterval(()=>void refresh(),15000);return()=>{alive.current=false;++readRevision.current;controller.abort();clearInterval(timer);};},[refreshKey]);
 useEffect(()=>{if(!code)return;const timer=window.setTimeout(()=>setCode(null),Math.max(0,code.expires*1000-Date.now()));return()=>clearTimeout(timer);},[code]);
 async function save(server:Server,node:Node|undefined,action:'save'|'pause'|'rotate'){
  if(node&&!confirm('这会更新该服务器的语音路线；停用、切换路线或轮换密钥会断开该节点的网页通话。继续吗？'))return;
  setBusy(true);++readRevision.current;setCode(null);setMessage('');
  try{const v=await api<{message:string}>('/admin/voice-nodes',{id:node?.id??ids[server.id],server:server.id,enabled:action==='pause'?!node?.enabled:true,use_gateway:action==='save'?(choices[server.id]??(node&&!node.use_gateway?'node':'site'))==='site':node?.use_gateway===true,revision:node?.revision??0,rotate:action==='rotate',password});
   if(!alive.current)return;setMessage(v.message);setChoices(v=>{const n={...v};delete n[server.id];return n;});await load();await onSaved?.();
  }catch(e){if(alive.current)setMessage(e instanceof Error?e.message:'路线保存失败');}finally{if(alive.current){setBusy(false);setPassword('');}}
 }
 async function issue(id:string){setBusy(true);setCode(null);setMessage('');try{const value=await api<Code>('/admin/voice-nodes/pair',{id,password});if(alive.current)setCode(value);}catch(e){if(alive.current)setMessage(e instanceof Error?e.message:'配对码生成失败');}finally{if(alive.current){setBusy(false);setPassword('');}}}
 return <section className="admin-settings"><h3>服务器走哪条路线</h3><p>主站网关：语音由网站所在机器转发。同机节点：语音直达 TS 所在机器，网站只负责账号和控制。</p><small>先在下方保存服务器，再为它选择路线。节点离线会提示失败，不会偷偷改走主站。更改设置需要验证管理员密码。</small>
 {message&&<p role="status">{message}</p>}<label>验证管理员密码<input type="password" autoComplete="current-password" maxLength={128} value={password} onChange={e=>setPassword(e.target.value)}/></label>
 {servers.length===0&&<p>保存服务器后，这里会显示每台服务器的连接路线。</p>}
 {servers.map(server=>{const node=nodes.find(n=>n.server===server.id),choice=choices[server.id]??(node&&!node.use_gateway?'node':'site');return <fieldset key={server.id} disabled={busy}><legend>{server.name}</legend><p className="muted">{server.address}</p><p><strong>当前：{!node||node.use_gateway?'主站网关转发':!node.enabled?'节点已停用':`同机节点 · ${node.id}`}</strong>{node&&!node.use_gateway&&node.enabled&&` · ${node.idle>0?'在线':'离线，不能连接'}`}</p>
 <label>语音连接路线<select value={choice} onChange={e=>{setChoices(v=>({...v,[server.id]:e.target.value}));if(!node&&!ids[server.id])setIds(v=>({...v,[server.id]:'node-'+crypto.randomUUID().slice(0,8)}));}}><option value="site">主站网关转发</option><option value="node">同机节点 · 语音直达 TS 机器</option></select></label>
 {choice==='node'&&!node&&<label>节点标识（已自动生成，安装时填写）<input value={ids[server.id]??''} maxLength={64} pattern="[A-Za-z0-9_-]+" onChange={e=>setIds(v=>({...v,[server.id]:e.target.value}))}/></label>}
 <button type="button" className="primary" disabled={!password||(!node&&(choice==='site'||!ids[server.id]))} onClick={()=>void save(server,node,'save')}>保存路线</button>
 {node&&!node.use_gateway&&<><p>节点安装时选择：主站网址、本节点标识 <strong>{node.id}</strong>、下方生成的配对码。只交给可信 TS 机器的管理员。</p><div className="split-actions"><button type="button" className="secondary" disabled={!password||!node.enabled} onClick={()=>void issue(node.id)}>生成安装配对码</button><button type="button" className="secondary" disabled={!password} onClick={()=>void save(server,node,'pause')}>{node.enabled?'停用节点':'重新启用'}</button><button type="button" className="text-button danger" disabled={!password} onClick={()=>void save(server,node,'rotate')}>轮换密钥并撤销旧连接</button></div></>}
 </fieldset>;})}
 {code&&<fieldset><legend>复制到节点安装窗口 · 5分钟有效</legend><p>节点标识：{code.node} · 主站：{code.main_url}</p><label>一次性配对码<input type="password" readOnly value={code.code} autoComplete="off" spellCheck={false}/></label><button type="button" className="secondary" onClick={()=>void navigator.clipboard.writeText(code.code).then(()=>setMessage('配对码已复制。')).catch(()=>setMessage('剪贴板不可用，请检查浏览器权限。'))}>复制配对码</button><button type="button" className="text-button" onClick={()=>setCode(null)}>隐藏</button><small>领取一次即失效。账号退出、权限撤销、节点停用或密钥轮换后，旧授权不能继续使用。</small></fieldset>}
 </section>;
}
