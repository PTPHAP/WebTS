import {useEffect,useState} from 'react';
import {api} from './api';
type Storage={enabled:boolean;endpoint:string;bucket:string;region:string;retention_days:number;credentials_set:boolean};
export function AdminStorage(){
  const[value,setValue]=useState<Storage>(),[access,setAccess]=useState(''),[secret,setSecret]=useState(''),[password,setPassword]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState(false);
  useEffect(()=>{api<Storage>('/admin/storage').then(setValue).catch(e=>setMessage(e.message));},[]);
  if(!value)return <p role="status">{message||'正在读取私有存储配置…'}</p>;
  return <form className="admin-settings" onSubmit={async e=>{e.preventDefault();setBusy(true);try{const r=await api<{message:string}>('/admin/storage',{password,storage:{enabled:value.enabled,endpoint:value.endpoint,bucket:value.bucket,region:value.region,retention_days:value.retention_days,access_key:access,secret_key:secret}});setMessage(r.message);setAccess('');setSecret('');setValue(await api<Storage>('/admin/storage'));}catch(e){setMessage(e instanceof Error?e.message:'保存失败');}finally{setBusy(false);setPassword('');}}}>
    <h3>好友私信与临时图片 · S3 兼容对象存储</h3><p className="muted">仅存端到端加密的密文，再以部署密钥增加 AES-256-GCM 存储加密。数据库只保存双方账号、消息编号、时间和存储索引。网站不识别正文，不能自动判断“敏感内容”。</p>
    {message&&<p className="auth-help" role="status">{message}</p>}<fieldset disabled={busy}>
    <label className="checkbox"><input type="checkbox" checked={value.enabled} onChange={e=>setValue({...value,enabled:e.target.checked})}/>启用临时加密私信存储</label>
    <label>公网 S3 API 端点<input placeholder="https://s3.example.com" type="url" value={value.endpoint} onChange={e=>setValue({...value,endpoint:e.target.value})} required={value.enabled}/></label><small>使用路径式寻址，仅 HTTPS 443，不使用 CDN、公共桶或预签名下载地址。内网、跳转及代理端点会被拒绝。</small>
    <label>私有桶名称<input value={value.bucket} maxLength={63} onChange={e=>setValue({...value,bucket:e.target.value})} required={value.enabled}/></label><label>Region<input value={value.region} placeholder="us-east-1" maxLength={64} onChange={e=>setValue({...value,region:e.target.value})} required={value.enabled}/></label>
    <label>Access Key<input type="password" autoComplete="new-password" value={access} onChange={e=>setAccess(e.target.value)} placeholder={value.credentials_set?'已保存，留空保持原值':'仅此桶和固定前缀的访问密钥'} required={value.enabled&&!value.credentials_set}/></label><label>Secret Key<input type="password" autoComplete="new-password" value={secret} onChange={e=>setSecret(e.target.value)} maxLength={1024} placeholder={value.credentials_set?'已保存，留空保持原值':'访问密钥对应的Secret Key'} required={value.enabled&&!value.credentials_set}/></label>
    <label>默认保留天数（1–30）<input type="number" min={1} max={30} value={value.retention_days} onChange={e=>setValue({...value,retention_days:Number(e.target.value)})}/></label>
    <p className="auth-help">在存储供应商后台：关闭公共访问；仅授予 webts-temporary/* 前缀的 PutObject/GetObject/DeleteObject 权限；配置与保留期一致的生命周期，并清理非当前版本和副本。关闭新发送不删除现有密文，过期后不可读取并继续尝试删除。有旧对象时不允许迁移桶。阅后即焚在对方成功解密后立即禁止读取，删除失败会重试。</p>
    <label>确认管理员密码<input type="password" autoComplete="current-password" required value={password} onChange={e=>setPassword(e.target.value)}/></label><button className="primary full" disabled={busy}>{busy?'保存中…':'保存并热加载'}</button></fieldset><a href="https://github.com/PTPHAP/WebTS/blob/main/docs/FRIENDS.md" target="_blank" rel="noreferrer">配置及加密边界说明 ↗</a>
  </form>;
}
