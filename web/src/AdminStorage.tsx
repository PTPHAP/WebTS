import {useEffect,useState} from 'react';
import {api} from './api';
type Storage={enabled:boolean;endpoint:string;bucket:string;region:string;retention_days:number;credentials_set:boolean;objects?:{total:number;pending:number;expired:number;probes:number;community?:number}};
type Entry={id:string;name:string;storage:Storage;objects:number};
type Stores={active:string;profiles:Entry[]};
const empty:Storage={enabled:false,endpoint:'',bucket:'',region:'',retention_days:7,credentials_set:false};
type Check={ok:boolean;message:string;cleanup_pending:boolean;roundtrip_match:boolean;stages:{stage:string;ok:boolean;status?:number;code?:string;message?:string}[]};
export function AdminStorage(){
  const[data,setData]=useState<Stores>(),[selected,setSelected]=useState('default'),[revision,setRevision]=useState(0),[error,setError]=useState(''),[saving,setSaving]=useState(false);
  useEffect(()=>{let alive=true;const abort=new AbortController(),timeout=setTimeout(()=>abort.abort(),10000);setData(undefined);setError('');api<Stores>('/admin/storage/profiles',undefined,undefined,abort.signal).then(value=>{if(alive)setData(value);}).catch(e=>{if(alive)setError(e.message);}).finally(()=>clearTimeout(timeout));return()=>{alive=false;clearTimeout(timeout);abort.abort();};},[revision]);
  if(!data)return <div role="status">{error||'正在读取对象存储配置…'}{error&&<button className="secondary" onClick={()=>setRevision(v=>v+1)}>重试</button>}</div>;
  const choice=selected==='new'&&data.profiles.length<8||data.profiles.some(p=>p.id===selected)?selected:'default';
  const entry=data.profiles.find(p=>p.id===choice)??{id:'',name:'新存储',storage:empty,objects:0};
  return <section className="storage-manager"><label>选择存储配置<select disabled={saving} value={choice} onChange={e=>setSelected(e.target.value)}>{data.profiles.map(p=><option key={p.id} value={p.id}>{p.name}{p.id===data.active?' · 默认写入':''} · {p.objects} 个对象索引</option>)}{data.profiles.length<8&&<option value="new">＋ 添加对象存储</option>}</select></label><p className="muted">最多8个配置。新对象写入默认配置；旧对象始终从原桶读取和清理。关闭某个配置的新发送不会停止读取与到期清理。当前默认：{data.profiles.find(p=>p.id===data.active)?.name}</p><StorageEditor key={entry.id||'new'} entry={entry} active={data.active} onBusy={setSaving} onSaved={id=>{setSelected(id);setRevision(v=>v+1);}}/></section>;
}
function StorageEditor({entry,active,onBusy,onSaved}:{entry:Entry;active:string;onBusy:(value:boolean)=>void;onSaved:(id:string)=>void}){
  const[value,setValue]=useState<Storage>(entry.storage),[access,setAccess]=useState(''),[secret,setSecret]=useState(''),[password,setPassword]=useState(''),[message,setMessage]=useState(''),[busy,setBusy]=useState(false),[dirty,setDirty]=useState(false),[check,setCheck]=useState<Check>();
  const[name,setName]=useState(entry.name);
  if(!value)return <p role="status">{message||'正在读取私有存储配置…'}</p>;
  let rainyun=false;try{rainyun=new URL(value.endpoint).hostname.endsWith('.rains3.com');}catch{/* The server validates the completed endpoint. */}
  function change(fields:Partial<Storage>){setValue(v=>v?{...v,...fields}:v);setDirty(true);setCheck(undefined);}
  async function run(test:boolean,action='save'){if(!value||busy)return;setBusy(true);onBusy(true);setMessage('');setCheck(undefined);try{
    if(test){const result=await api<Check>('/admin/storage/test',{password,store_id:entry.id});setCheck(result);setMessage(result.message);}
    else{const result=await api<{message:string;id:string}>('/admin/storage/profiles',{password,id:entry.id,name,action,storage:{enabled:value.enabled,endpoint:value.endpoint,bucket:value.bucket,region:value.region,retention_days:value.retention_days,access_key:access,secret_key:secret}});setMessage(result.message);setAccess('');setSecret('');setDirty(false);onSaved(action==='delete'?'default':result.id);}

  }catch(e){setMessage(e instanceof Error?e.message:test?'检测失败':'保存失败');}finally{setBusy(false);onBusy(false);setPassword('');}}
  return <form className="admin-settings storage-settings" onSubmit={e=>{e.preventDefault();void run(false);}}>
    <h3>{entry.id?entry.name:'添加对象存储'}</h3><p className="muted">支持 S3 兼容服务，包括雨云 ROS。两种私信模式均使用 AES-256-GCM 存储密文；默认普通聊天，无需额外确认；本站可以解密普通消息，端到端发送需设备验证且由用户选择。数据库保存账号、时间、加密模式和对象索引。</p>
    {message&&<p className="auth-help" role="status">{message}</p>}
    {value.objects&&<div className="storage-summary"><span>临时索引 <strong>{value.objects.total}</strong></span><span>待发送 <strong>{value.objects.pending}</strong></span><span>待过期清理 <strong>{value.objects.expired}</strong></span>{(value.objects.community??0)>0&&<span>社区内容对象 <strong>{value.objects.community}</strong></span>}{value.objects.probes>0&&<span>待清理检测对象 <strong>{value.objects.probes}</strong></span>}</div>}
    {check&&<section className={`storage-check ${check.ok?'passed':''}`} aria-label="存储检测结果">{check.stages.map(s=><article key={s.stage}><strong>{s.ok?'✓':'!'} {s.stage}{s.status?` · HTTP ${s.status}`:''}{s.code?` · ${s.code}`:''}</strong><p>{s.message??'通过'}</p></article>)}{check.cleanup_pending&&<p>检测文件尚未确认删除，后台会重试；请先修复删除权限。</p>}</section>}
    <fieldset disabled={busy}>{entry.id!=='default'&&<label>配置名称<input value={name} maxLength={40} required onChange={e=>{setName(e.target.value);setDirty(true);}}/></label>}
      <label className="checkbox"><input type="checkbox" checked={value.enabled} onChange={e=>change({enabled:e.target.checked})}/>允许此配置接收新对象</label>
      <label>存储服务<select value={rainyun?'rainyun':'s3'} onChange={e=>{if(e.target.value==='rainyun')change({endpoint:'https://cn-sy1.rains3.com',region:'us-east-1'});else change({endpoint:'',region:''});}}><option value="s3">其他 S3 兼容服务</option><option value="rainyun">雨云 ROS</option></select></label>
      <label>{rainyun?'雨云 S3 API 端点':'公网 S3 API 端点'}<input placeholder={rainyun?'https://cn-sy1.rains3.com':'https://s3.example.com'} type="url" value={value.endpoint} onChange={e=>change({endpoint:e.target.value})} required={value.enabled}/></label><small>填写控制台提供的 API Endpoint，不含桶名或文件路径；路径式寻址，仅 HTTPS 443。公共下载域名、CDN、内网和跳转端点不能使用。</small>
      <label>私有桶名称<input value={value.bucket} maxLength={63} onChange={e=>change({bucket:e.target.value})} required={value.enabled} placeholder="准确的 Bucket 名称，不是 Access Key"/></label>
      <label>{rainyun?'签名 Region（可留空）':'Region'}<input value={value.region} placeholder="us-east-1" maxLength={64} onChange={e=>change({region:e.target.value})} required={value.enabled&&!rainyun}/></label>
      {rainyun&&<p className="auth-help">雨云控制台未提供 Region 时可以留空，保存会使用 us-east-1 作为 S3 签名参数，不会把数据迁往美国。桶的实际位置由雨云端点决定；不要填账号编号。已有临时对象时仍可修正同桶 Region。</p>}
      <label>Access Key<input type="password" autoComplete="new-password" value={access} onChange={e=>{setAccess(e.target.value);setDirty(true);}} maxLength={256} placeholder={value.credentials_set?'已保存，留空保持原值':'仅此桶和固定前缀的访问密钥'} required={value.enabled&&!value.credentials_set}/></label>
      <label>Secret Key<input type="password" autoComplete="new-password" value={secret} onChange={e=>{setSecret(e.target.value);setDirty(true);}} maxLength={1024} placeholder={value.credentials_set?'已保存，留空保持原值':'Access Key 对应的 Secret Key'} required={value.enabled&&!value.credentials_set}/></label>
      <label>默认保留天数（1–30）<input type="number" min={1} max={30} value={value.retention_days} onChange={e=>change({retention_days:Number(e.target.value)})}/></label>
      <p className="auth-help">桶保持私有；访问密钥只需 webts-temporary/* 的 PutObject、GetObject、DeleteObject 权限。IP 白名单须允许部署服务器。配置生命周期清理过期文件及非当前版本。关闭新发送后仍会清理过期密文；新增配置后可切换默认写入；已有对象的配置不能修改端点或桶名、不能删除，同桶修正 Region 和轮换凭据可以保存。各桶都需要配置相同的私有访问与清理规则。</p>
      <label>确认管理员密码<input type="password" autoComplete="current-password" required value={password} onChange={e=>setPassword(e.target.value)} maxLength={128}/></label>
      <div className="split-actions"><button className="primary" disabled={busy}>{busy?'处理中…':'保存并热加载'}</button><button type="button" className="secondary" disabled={busy||dirty||!password||!value.credentials_set||!entry.id} onClick={()=>void run(true)}>检测已保存配置</button>{entry.id&&entry.id!==active&&<button type="button" className="secondary" disabled={busy||dirty||!password} onClick={()=>void run(false,'activate')}>设为默认写入</button>}{entry.id&&entry.id!=='default'&&entry.id!==active&&<button type="button" className="text-button danger" disabled={busy||dirty||!password||entry.objects>0} onClick={()=>{if(confirm('删除这个没有对象的存储配置？不会删除其他配置。'))void run(false,'delete');}}>删除配置</button>}</div><small>检测仅上传约 100 字节的加密测试文件，再读取和删除；不读取用户消息。修改配置后先保存，检测时再次输入密码。</small>
    </fieldset><a href="https://github.com/PTPHAP/WebTS/blob/main/docs/OBJECT-STORAGE.md" target="_blank" rel="noreferrer">雨云及其他 S3 服务配置说明 ↗</a>
  </form>;
}
