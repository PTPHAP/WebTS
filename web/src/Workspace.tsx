import {useAccountPreference} from './account-preferences';
import {Children,useRef,useState} from 'react';
import type {CSSProperties,ReactNode} from 'react';
import {defaultLayout,panelIds,readLayout,reorderPanels,resizePanels} from './workspace-layout';
import type {Layout,PanelId} from './workspace-layout';
const labels={channels:'频道树',chat:'聊天与语音',members:'频道与成员资料'};
export function Workspace({mobile,children}:{mobile:string;children:ReactNode}) {
  const [layout,setLayout]=useAccountPreference('layout',()=>{try{return readLayout(JSON.parse(localStorage.getItem('webts-layout')??'null'));}catch{return readLayout(null);}});
  const [editing,setEditing]=useState(false),[saved,setSaved]=useState(true);
  const dragged=useRef<PanelId|null>(null),resizing=useRef<{index:number;x:number;width:number;layout:Layout}|null>(null);
  const root=useRef<HTMLElement>(null);
  function update(next:Layout){setLayout(next);try{localStorage.setItem('webts-layout',JSON.stringify(next));setSaved(true);}catch{setSaved(false);}}
  const nodes=Children.toArray(children);
  return <div className="workspace-shell"><div className="layout-toolbar"><button className="text-button" aria-pressed={editing} onClick={()=>setEditing(!editing)}>{editing?'完成布局':'自定义布局'}</button>{editing&&<><span>拖动模块标题换位，拖动分隔线调宽；也可使用下方按钮。</span><button className="text-button" onClick={()=>update(readLayout(defaultLayout))}>恢复默认布局</button></>}{!saved&&<span role="status">布局已生效，浏览器禁止保存。</span>}</div><main ref={root} className={`workspace mobile-${mobile} ${editing?'layout-editing':''}`} style={{gridTemplateColumns:layout.order.map(id=>`minmax(0,${layout.widths[id]}fr)`).join(' ')} as CSSProperties}>
    {layout.order.map((id,index)=><div key={id} className={`workspace-pane pane-${id}`} onDragOver={e=>{if(dragged.current)e.preventDefault();}} onDrop={e=>{if(!dragged.current)return;e.preventDefault();update(reorderPanels(layout,dragged.current,id));dragged.current=null;}}>
      {editing&&<div className="layout-handle"><button draggable onDragStart={e=>{dragged.current=id;e.dataTransfer.setData('text/plain',id);e.dataTransfer.effectAllowed='move';}} onDragEnd={()=>dragged.current=null} aria-label={`拖动${labels[id]}调整位置`}>⠿ {labels[id]}</button><button disabled={index===0} aria-label={`${labels[id]}前移`} onClick={()=>update(reorderPanels(layout,id,layout.order[index-1]))}>←</button><button disabled={index===2} aria-label={`${labels[id]}后移`} onClick={()=>update(reorderPanels(layout,id,layout.order[index+1]))}>→</button></div>}
      {nodes[panelIds.indexOf(id)]}
    </div>)}
    {editing&&[0,1].map(index=><div key={index} className="layout-separator" role="separator" aria-orientation="vertical" aria-label={`调整${labels[layout.order[index]]}宽度`} aria-valuemin={layout.order[index]==='chat'?30:15} aria-valuemax={layout.widths[layout.order[index]]+layout.widths[layout.order[index+1]]-(layout.order[index+1]==='chat'?30:15)} aria-valuenow={Math.round(layout.widths[layout.order[index]])} tabIndex={0} style={{left:`${layout.order.slice(0,index+1).reduce((sum,id)=>sum+layout.widths[id],0)}%`}} onKeyDown={e=>{if(['ArrowLeft','ArrowRight'].includes(e.key)){e.preventDefault();update(resizePanels(layout,index,e.key==='ArrowRight'?2:-2));}}} onPointerDown={e=>{if(e.button!==0||!root.current)return;e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);resizing.current={index,x:e.clientX,width:root.current.getBoundingClientRect().width,layout};}} onPointerMove={e=>{const start=resizing.current;if(start&&e.currentTarget.hasPointerCapture(e.pointerId))update(resizePanels(start.layout,start.index,(e.clientX-start.x)/start.width*100));}} onPointerUp={()=>resizing.current=null} onPointerCancel={()=>resizing.current=null} onLostPointerCapture={()=>resizing.current=null}/>) }
  </main></div>;
}
