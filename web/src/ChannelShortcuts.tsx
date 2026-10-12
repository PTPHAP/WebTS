import {useEffect,useRef,useState} from 'react';
import {invitation,type Favorite} from './favorites';
export function ChannelShortcuts({items,servers,join,remove}:{items:Favorite[];servers:{id:string;name:string}[];join:(v:Favorite)=>void;remove:(v:Favorite)=>void}){
  return <details className="channel-shortcuts"><summary>收藏与快速加入 <small>{items.length}/8</small></summary>{items.length?items.map((v,i)=><div className="shortcut-row" key={`${v.server}:${v.uid}:${v.channel}:${i}`}><button className="text-button" disabled={!servers.some(s=>s.id===v.server)} onClick={()=>join(v)}><strong>{v.name}</strong><small>{servers.find(s=>s.id===v.server)?.name??'服务器已移除'}</small></button><button className="icon-button" aria-label={`移除收藏 ${v.name}`} onClick={()=>remove(v)}>×</button></div>):<p className="muted">通过频道菜单收藏；跨设备随账号同步。</p>}</details>;
}
export function ShareChannel({target}:{target:Favorite}){
  const canvas=useRef<HTMLCanvasElement>(null),[message,setMessage]=useState(''),[error,setError]=useState('');const link=invitation(target,location.origin);
  useEffect(()=>{let alive=true;void import('qrcode').then(q=>{if(alive&&canvas.current)return q.toCanvas(canvas.current,link,{width:220,margin:2,errorCorrectionLevel:'M'});}).catch(()=>{if(alive)setError('二维码暂不可用，请复制链接。');});return()=>{alive=false;};},[link]);
  return <section className="channel-share"><h3>{target.name}</h3><canvas ref={canvas} aria-label="频道邀请二维码"/><label>邀请链接<input readOnly value={link} onFocus={e=>e.currentTarget.select()}/></label><button className="primary full" onClick={()=>void navigator.clipboard.writeText(link).then(()=>setMessage('链接已复制')).catch(()=>setMessage('请选择上面的链接并复制'))}>复制邀请链接</button>{navigator.share&&<button className="secondary full" onClick={()=>void navigator.share({title:'WebTS 频道邀请',url:link}).catch(()=>{})}>系统分享</button>}<p role="status">{error||message}</p><small>链接不含密码或身份密钥。收件人需登录、选择自己的身份，仍由 TS 验证权限和频道密码；仅支持本站配置的服务器。</small></section>;
}
