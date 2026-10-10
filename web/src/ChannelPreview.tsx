import {useAccountPreference} from './account-preferences';
import {useRef} from 'react';
import type {ReactNode} from 'react';
export function previewHeight(value:unknown){return typeof value==='number'&&Number.isFinite(value)?Math.max(80,Math.min(420,value)):200;}
export function ChannelPreview({header,children}:{header:ReactNode;children:ReactNode}){
  const[height,setHeight]=useAccountPreference('previewHeight',()=>{try{return previewHeight(JSON.parse(localStorage.getItem('webts-preview-height')??'null'));}catch{return 200;}});
  const start=useRef<{y:number;height:number;max:number}|null>(null);
  function update(value:number){const next=previewHeight(value);setHeight(next);try{localStorage.setItem('webts-preview-height',String(next));}catch{}}
  return <section className="channel-announcement resizable-preview" aria-label="预览频道介绍" style={{height}}><header className="channel-preview-header">{header}</header><div className="channel-preview-content">{children}</div><div role="separator" aria-orientation="horizontal" aria-label="调整频道介绍高度" aria-valuemin={80} aria-valuemax={420} aria-valuenow={Math.round(height)} tabIndex={0} className="preview-resizer" onKeyDown={e=>{if(e.key==='ArrowUp'||e.key==='ArrowDown'){e.preventDefault();update(height+(e.key==='ArrowUp'?-10:10));}}} onPointerDown={e=>{if(e.button!==0)return;e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);const section=e.currentTarget.parentElement!;start.current={y:e.clientY,height:section.getBoundingClientRect().height,max:Math.max(80,Math.min(420,(section.parentElement?.clientHeight??700)*.45))};}} onPointerMove={e=>{const s=start.current;if(s&&e.currentTarget.hasPointerCapture(e.pointerId))update(Math.min(s.max,s.height+e.clientY-s.y));}} onPointerUp={()=>start.current=null} onPointerCancel={()=>start.current=null} onLostPointerCapture={()=>start.current=null}/></section>;
}
