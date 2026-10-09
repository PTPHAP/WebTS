import {useEffect,useRef,useState} from 'react';
import type {FormEvent} from 'react';
import {api} from './api';
import {avatarImage} from './avatar';
export type Profile={display_name:string;about:string;avatar:string;sync_avatar:boolean;sync_about:boolean};
export const emptyProfile:Profile={display_name:'',about:'',avatar:'',sync_avatar:false,sync_about:false};
export function ProfileSettings({profile,email,onSaved}:{profile:Profile;email:string;onSaved:(value:Profile)=>void}){
  const[value,setValue]=useState(profile),[busy,setBusy]=useState(false),[imageBusy,setImageBusy]=useState(false),[message,setMessage]=useState('');
  const revision=useRef(0);useEffect(()=>()=>{revision.current++;},[]);
  async function upload(file:File){const id=++revision.current;setImageBusy(true);setMessage('');try{const avatar=await avatarImage(file);if(id===revision.current)setValue(v=>({...v,avatar,sync_avatar:true}));}catch(e){if(id===revision.current)setMessage(e instanceof Error?e.message:'图片处理失败');}finally{if(id===revision.current)setImageBusy(false);}}
  async function save(e:FormEvent){e.preventDefault();setBusy(true);setMessage('');try{const data=await api<Profile>('/profile',value);setValue(data);onSaved(data);setMessage('资料已保存。昵称作为下次连接的默认值；头像和介绍按同步开关尝试应用到当前连接。');}catch(e){setMessage(e instanceof Error?e.message:'保存失败');}finally{setBusy(false);}}
  return <form className="profile-settings" onSubmit={save}>
    <div className="profile-cover"><span className="avatar large">{value.avatar?<img src={`data:image/png;base64,${value.avatar}`} alt="我的头像"/>:(value.display_name||email)[0]}</span><div><h3>我的声音名片</h3><p>一份资料，在不同服务器延续。</p></div></div>
    <p className="auth-help" role="status">{message||'资料随邮箱账号保存。头像与介绍同步由目标 TeamSpeak 服务器检查权限。'}</p>
    <fieldset disabled={busy||imageBusy}><legend>基本资料</legend>
      <label>登录邮箱<input value={email} readOnly autoComplete="email"/></label><small>登录邮箱仅自己和站点管理员可见，不同步给 TS。</small>
      <label>常用昵称<input value={value.display_name} onChange={e=>setValue({...value,display_name:e.target.value})} minLength={3} maxLength={30} placeholder="留空使用所选身份名称"/></label>
      <label>个人介绍<textarea value={value.about} onChange={e=>setValue({...value,about:e.target.value})} maxLength={512} rows={4} placeholder="让频道里的朋友更了解你"/></label><small>{new TextEncoder().encode(value.about).length} / 512 字节 · 中文通常每字3字节</small>
      <label>固定头像<input type="file" accept="image/png,image/jpeg,image/webp" onChange={e=>{const file=e.target.files?.[0];e.target.value='';if(file)void upload(file);}}/></label>
      {value.avatar&&<button type="button" className="text-button danger" onClick={()=>setValue({...value,avatar:''})}>移除账号默认头像</button>}
    </fieldset>
    <fieldset disabled={busy||imageBusy}><legend>TeamSpeak 客户端互通</legend><label className="checkbox"><input type="checkbox" checked={value.sync_avatar} onChange={e=>setValue({...value,sync_avatar:e.target.checked})}/>连接时使用账号头像</label><label className="checkbox"><input type="checkbox" checked={value.sync_about} onChange={e=>setValue({...value,sync_about:e.target.checked})}/>连接时同步个人介绍（覆盖服务器原介绍）</label><small>同步只修改当前身份的公开资料，不改变 UID 或权限。关闭开关或移除默认头像会保留服务器已有资料。TS3 头像文件传输不加密，请选择可公开的图片；PNG / JPEG / WebP 最大2MiB，保存前裁切、压缩并清除元数据。</small></fieldset>
    <button className="primary full" disabled={busy||imageBusy||new TextEncoder().encode(value.about).length>512}>{imageBusy?'正在处理图片…':busy?'正在保存…':'保存个人资料'}</button>
  </form>;
}
