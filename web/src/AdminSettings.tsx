import {useEffect,useState} from 'react';
import type {FormEvent} from 'react';
import {api} from './api';
import {AdminHome} from './SiteHome';
import type {Home} from './SiteHome';
import {AdminAccounts} from './AdminAccounts';
import {AdminNotices} from './AdminNotices';
import {defaultImageLimits,imageLimitFields,readImageLimits} from './image-limits';
import type {ImageLimits} from './image-limits';

export function AdminPanel({onSaved,currentEmail,onHomeSaved}:{onSaved:()=>Promise<void>;currentEmail:string;onHomeSaved:(home:Home)=>void}){
  const[tab,setTab]=useState('accounts');
  return <><div className="admin-tabs" role="tablist" aria-label="站点管理分类"><button role="tab" aria-selected={tab==='accounts'} onClick={()=>setTab('accounts')}>账号管理</button><button role="tab" aria-selected={tab==='settings'} onClick={()=>setTab('settings')}>基础配置</button><button role="tab" aria-selected={tab==='notices'} onClick={()=>setTab('notices')}>信件与通知</button><button role="tab" aria-selected={tab==='home'} onClick={()=>setTab('home')}>外观、协议与公告</button></div><div role="tabpanel">{tab==='accounts'?<AdminAccounts currentEmail={currentEmail}/>:tab==='notices'?<AdminNotices/>:tab==='home'?<AdminHome onSaved={onHomeSaved}/>:<AdminSettings onSaved={onSaved}/>}</div></>;
}

type Target={id:string;name:string;address:string};
type Smtp={host:string;port:number;username:string;from:string;password?:string;password_set?:boolean};
type Settings={servers:Target[];default_server:string;allow_custom:boolean;smtp:Smtp|null;image_limits:ImageLimits};
const emptySmtp:Smtp={host:'',port:465,username:'',from:''};
export function AdminSettings({onSaved}:{onSaved:()=>Promise<void>}){
  const[settings,setSettings]=useState<Settings|null>(null);const[smtp,setSmtp]=useState<Smtp>(emptySmtp);const[enabled,setEnabled]=useState(false);const[password,setPassword]=useState('');const[secret,setSecret]=useState('');const[busy,setBusy]=useState(false);const[message,setMessage]=useState('');const[failed,setFailed]=useState(false);
  useEffect(()=>{api<Settings>('/admin/settings').then(s=>{setSettings({...s,image_limits:readImageLimits(s.image_limits)});setSmtp(s.smtp??emptySmtp);setEnabled(s.smtp!==null);}).catch(e=>{setFailed(true);setMessage(e.message);});},[]);
  async function save(e:FormEvent){e.preventDefault();if(!settings)return;setBusy(true);setMessage('');try{const mail=enabled?{host:smtp.host,port:smtp.port,username:smtp.username,from:smtp.from,password:secret}:null;const result=await api<{message:string}>('/admin/settings',{password,settings:{...settings,smtp:mail}});setMessage(result.message);setFailed(false);setSmtp({...smtp,password_set:enabled});setSecret('');await onSaved();}catch(e){setFailed(true);setMessage(e instanceof Error?e.message:'保存失败');}finally{setBusy(false);setPassword('');}}
  if(!settings)return <p role="status">{message||'正在读取站点设置…'}</p>;
  function target(index:number,field:'name'|'address',value:string){setSettings(s=>s&&({...s,servers:s.servers.map((row,i)=>i===index?{...row,[field]:value}:row)}));}
  return <form className="admin-settings" onSubmit={save}>
    <p className="muted">保存后立即生效，无需重启。网站管理员不会获得额外的 TeamSpeak 权限。</p>
    {message&&<p className={failed?'auth-error':'auth-help'} role="status">{message}</p>}
    <fieldset disabled={busy}><legend>服务器与连接</legend>
      {settings.servers.map((row,index)=><article className="identity-card" key={row.id}><label>服务器名称 {index+1}<input value={row.name} onChange={e=>target(index,'name',e.target.value)} maxLength={80} required/></label><label>公网地址 {index+1}<input value={row.address} onChange={e=>target(index,'address',e.target.value)} placeholder="ts.example.com:9987" maxLength={253} required/></label><button type="button" className="text-button danger" onClick={()=>setSettings(s=>s&&({...s,servers:s.servers.filter(r=>r.id!==row.id),default_server:s.default_server===row.id?'':s.default_server}))}>移除该服务器</button></article>)}
      <button type="button" className="secondary full" disabled={settings.servers.length>=32} onClick={()=>setSettings({...settings,servers:[...settings.servers,{id:crypto.randomUUID(),name:'',address:''}]})}>添加服务器</button>
      <label>默认连接服务器<select value={settings.default_server} onChange={e=>setSettings({...settings,default_server:e.target.value})}><option value="">不指定</option>{settings.servers.map(row=><option value={row.id} key={row.id}>{row.name||'未命名服务器'}</option>)}</select></label>
      <label className="checkbox"><input type="checkbox" checked={settings.allow_custom} onChange={e=>setSettings({...settings,allow_custom:e.target.checked})}/>允许已登录用户连接自定义公网地址</label>
      <small>关闭自定义连接、移除服务器或更改其地址时，对应的活动连接会断开。自定义目标仍必须强制加密并使用 Opus。</small>
    </fieldset>
    <fieldset disabled={busy}><legend>图片与头像</legend>
      <p className="muted">1KiB = 1024字节，1MiB = 1024KiB。保存后热加载，新传输使用新限制；已开始的传输按开始时配置完成。频道图片缓存会重新读取。</p>
      {imageLimitFields.map(({key,label,min,max})=><label key={key}>{label}<input type="number" min={min} max={max} step={1} required value={settings.image_limits[key]} onChange={e=>setSettings({...settings,image_limits:{...settings.image_limits,[key]:Number(e.target.value)}})}/><small>允许范围：{min}–{max}</small></label>)}
      <button className="secondary full" type="button" onClick={()=>setSettings({...settings,image_limits:{...defaultImageLimits}})}>恢复图片与头像默认限制</button>
      <small>头像裁剪后最长256px；频道图片显示最长1280px。TS服务器仍可施加更小的头像上传限制。现有账号头像保留；文件类型、权限、路径检查、解码内存与传输并发安全上限无法关闭。外部图片保持点击打开。</small>
    </fieldset>
    <fieldset disabled={busy}><legend>发信邮箱</legend>
      <label className="checkbox"><input type="checkbox" checked={enabled} onChange={e=>setEnabled(e.target.checked)}/>启用验证与找回邮件</label>
      {enabled&&<><label>SMTP 主机<input value={smtp.host} maxLength={253} required onChange={e=>setSmtp({...smtp,host:e.target.value})} placeholder="smtp.example.com"/></label><label>SMTP 端口<input type="number" min={1} max={65535} value={smtp.port} required onChange={e=>setSmtp({...smtp,port:Number(e.target.value)})}/></label><small>465 使用隐式 TLS，其他端口使用 STARTTLS；均验证证书。</small><label>SMTP 登录账号<input value={smtp.username} required maxLength={254} onChange={e=>setSmtp({...smtp,username:e.target.value})}/></label><label>发件人<input value={smtp.from} required maxLength={320} placeholder="WebTS <mailer@example.com>" onChange={e=>setSmtp({...smtp,from:e.target.value})}/></label><label>SMTP 授权码 / 密码<input type="password" value={secret} maxLength={1024} required={!smtp.password_set} autoComplete="new-password" placeholder={smtp.password_set?'已保存，留空保持原授权码':'填写邮箱服务的授权码'} onChange={e=>setSecret(e.target.value)}/></label><small>授权码加密保存，后台不会回显。网站服务具有解密和发送邮件的能力。</small></>}
    </fieldset>
    <label>验证当前管理员密码<input type="password" value={password} required autoComplete="current-password" onChange={e=>setPassword(e.target.value)} maxLength={128}/></label>
    <button className="primary full" disabled={busy}>{busy?'正在保存…':'保存并立即生效'}</button>
  </form>;
}
