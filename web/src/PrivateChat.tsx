import {useEffect,useRef} from 'react';
import type {ReactNode} from 'react';
import type {Member} from './api';

export function PrivateChat({members,peer,recipient,draft,setDraft,choose,send,children,count}:{members:Member[];peer?:Member;recipient?:Member;draft:string;setDraft:(text:string)=>void;choose:(member:Member)=>void;send:(text:string)=>boolean;children:ReactNode;count:number}){
  const log=useRef<HTMLDivElement>(null);
  useEffect(()=>{if(log.current)log.current.scrollTop=log.current.scrollHeight;},[count,peer?.id]);
  return <section className="private-chat" aria-label="独立私聊会话">
    <label className="private-recipient">聊天对象<select aria-label="私聊对象" value={peer?.id??''} onChange={e=>{const member=members.find(m=>m.id===Number(e.target.value));if(member)choose(member);}}><option value="" disabled>选择一位成员</option>{peer&&!recipient&&<option value={peer.id}>{peer.name} · 已离线</option>}{members.map(m=><option key={m.id} value={m.id}>{m.name}</option>)}</select></label>
    <div className="messages" ref={log} role="log" aria-label="私聊消息">{count?children:<div className="chat-empty"><p>{peer?`与 ${peer.name} 的私聊`:'选择一位成员，开始私聊'}</p><small>独立会话，不影响频道聊天；消息只在当前连接中保留。</small></div>}</div>
    {peer&&!recipient&&<p className="private-offline" role="status">对方已离线，暂时无法发送消息。</p>}
    <form className="private-composer composer" onSubmit={e=>{e.preventDefault();if(recipient&&send(draft))setDraft('');}}><input aria-label="私聊内容" placeholder={recipient?`私聊 ${recipient.name}`:'先选择在线成员'} value={draft} onChange={e=>setDraft(e.target.value)} maxLength={1024} disabled={!recipient}/><button aria-label="发送私聊消息" disabled={!recipient||!draft.trim()}>发送</button></form>
  </section>;
}
