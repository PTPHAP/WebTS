import {useRef,useState} from 'react';
import type {Channel} from './api';
export function channelPatch(data:FormData,target?:Channel):Record<string,unknown>{
  const out:Record<string,unknown>={};
  for(const key of ['name','topic','description','kind','codec','quality','maxClients','maxFamilyClients','neededTalkPower','order']){
    const raw=data.get(key);if(raw===null||raw==='')continue;
    const value=['quality','maxClients','maxFamilyClients','neededTalkPower','order'].includes(key)?Number(raw):raw;
    const previous=key==='codec'?(target?.codec===4?'voice':target?.codec===5?'music':undefined):target?.[key as keyof Channel];
    if(!target||value!==previous)out[key]=value;
  }
  // Empty topic/description are valid deliberate edits, but never overwrite an unloaded description.
  for(const key of ['topic','description'] as const)if(data.has(key)&&String(data.get(key))!==String(target?.[key]??''))out[key]=String(data.get(key));
  if(!target||data.get('changePassword')==='on')out.password=String(data.get('password')??'');
  if(!target){out.parent=Number(data.get('parent')??0);out.description=String(data.get('description')??'');}
  return out;
}
export function ChannelEditor({target:liveTarget,channels,busy,error,submit,refresh,parent=0}:{target?:Channel;channels:Channel[];busy:boolean;error:string;submit:(payload:Record<string,unknown>)=>void;refresh:()=>void;parent?:number}){
  const initial=useRef(liveTarget?{...liveTarget}:undefined);
  if(initial.current&&initial.current.description==null&&liveTarget?.description!=null)initial.current.description=liveTarget.description;
  const target=initial.current;
  const[descriptionDirty,setDescriptionDirty]=useState(false),[description,setDescription]=useState(target?.description??'');
  const editing=!!target,loaded=!editing||target.description!=null;
  return <form onSubmit={e=>{e.preventDefault();if(busy)return;submit(channelPatch(new FormData(e.currentTarget),initial.current));}}><fieldset disabled={busy} className="channel-editor-fields"><label>名称<input name="name" defaultValue={target?.name??''} required maxLength={80}/></label>{!editing&&<label>父频道<select name="parent" defaultValue={parent}><option value="0">顶层频道</option>{channels.map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label>}<label>话题<input name="topic" defaultValue={target?.topic??''} maxLength={128}/></label><label>频道类型<select name="kind" defaultValue={target?.kind??'temporary'}><option value="temporary">临时（无人时删除）</option><option value="semi">半永久</option><option value="permanent">永久</option></select></label><label>Opus 编码<select name="codec" defaultValue={target?.codec===5?'music':target?.codec===4||!editing?'voice':''}><option value="" disabled>保留服务器原编码</option><option value="voice">Opus Voice · 语音</option><option value="music">Opus Music · 音乐</option></select></label><label>编码质量（1–10）<input name="quality" type="number" min={1} max={10} defaultValue={target?.quality??(!editing?6:'')}/></label>
    {editing&&<><label>频道人数上限<input name="maxClients" type="number" min={-1} max={65535} defaultValue={target.maxClients??''}/><small>-1 为不限；留空保持原设置。</small></label><label>频道及子频道总人数上限<input name="maxFamilyClients" type="number" min={-2} max={65535} defaultValue={target.maxFamilyClients??''}/><small>-2 为继承，-1 为不限。</small></label><label>所需发言权<input name="neededTalkPower" type="number" min={0} max={2147483647} defaultValue={target.neededTalkPower??''}/></label><label>在此频道之后排列<select name="order" defaultValue={target.order}><option value={0}>同级最前</option>{channels.filter(c=>c.parent===target.parent&&c.id!==target.id).map(c=><option key={c.id} value={c.id}>{c.name}</option>)}</select></label></>}
    <label>描述<textarea name="description" disabled={!loaded} value={descriptionDirty?description:initial.current?.description??description} maxLength={4096} onChange={e=>{setDescriptionDirty(true);setDescription(e.target.value);}}/></label>{!loaded&&<p className="muted">介绍尚未读取，保存不会清空原内容。<button type="button" className="text-button" onClick={refresh}>重新读取介绍</button></p>}{editing&&<label className="checkbox"><input type="checkbox" name="changePassword"/>修改密码（留空表示移除密码）</label>}<label>频道密码<input name="password" type="password" autoComplete="off" maxLength={256}/></label></fieldset><p className="muted">仅提交改动项。TeamSpeak 服务器按当前身份授权；网站管理员不会获得额外权限。加密策略保持强制。</p>{error&&<p className="field-error" role="alert">{error}</p>}<button className="primary full" disabled={busy}>{busy?'等待 TeamSpeak 确认…':'保存频道'}</button></form>;
}
