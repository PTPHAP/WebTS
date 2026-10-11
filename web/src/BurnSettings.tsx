import {useRef} from 'react';
import {burnDurations} from './friend-crypto';

export function BurnSettings({enabled,seconds,disabled=false,onEnabled,onSeconds}:{enabled:boolean;seconds:number;disabled?:boolean;onEnabled:(value:boolean)=>void;onSeconds:(value:number)=>void}){
  const panel=useRef<HTMLDetailsElement>(null);
  const close=()=>{if(panel.current){panel.current.open=false;panel.current.querySelector('summary')?.focus();}};
  return <details ref={panel} className="friend-burn-settings" onToggle={()=>{if(panel.current?.open){const sheet=panel.current.querySelector<HTMLElement>('.friend-burn-sheet');if(sheet)sheet.scrollTop=0;}}} onKeyDown={e=>{if(e.key==='Escape'&&panel.current?.open){e.preventDefault();e.stopPropagation();close();}}}>
    <summary><span aria-hidden="true">◷</span>阅后即焚<span className={enabled?'burn-on':'muted'}>{enabled?burnDurations.find(([value])=>value===seconds)?.[1]??'1分钟':'关闭'}</span></summary>
    <section className="friend-burn-sheet" aria-label="阅后即焚设置">
      <header><div><strong>阅后即焚</strong><small>对方已读后开始倒计时</small></div><button type="button" className="icon-button" aria-label="收起阅后即焚设置" onClick={close}>×</button></header>
      <label className="checkbox"><input type="checkbox" checked={enabled} disabled={disabled} onChange={e=>onEnabled(e.target.checked)}/>为新消息开启定时焚毁</label>
      <div className="friend-burn-grid" role="group" aria-label="阅后即焚时长">{burnDurations.map(([value,label])=><button type="button" key={value} disabled={disabled} aria-pressed={seconds===value} onClick={()=>onSeconds(value)}>{label}</button>)}</div>
      <p>设置随账号同步，只影响之后发送的消息。消息也可能先达到服务器保留上限；无法阻止对方截图或保存。</p>
      {disabled&&<p role="status">待发消息保留原来的焚毁设置，完成发送或放弃后可调整。</p>}
      <button type="button" className="secondary full" onClick={close}>完成</button>
    </section>
  </details>;
}
