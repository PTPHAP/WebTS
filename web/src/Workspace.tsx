import {useAccountPreference} from './account-preferences';
import {Children,useEffect,useRef,useState} from 'react';
import type {CSSProperties,ReactNode} from 'react';
import {defaultLayout,panelIds,readLayout,reorderPanels,resizePanels,minimumWorkspaceWidth,readPanelOrder} from './workspace-layout';
import type {Layout,PanelId} from './workspace-layout';
const labels={channels:'频道树',chat:'聊天与语音',members:'频道与成员资料'};
export function Workspace({mobile,compact,editing,finish,mobileOrder,onMobileOrder,select,children}:{mobile:string;compact:boolean;editing:boolean;finish:()=>void;mobileOrder:PanelId[];onMobileOrder:(order:PanelId[])=>void;select:(id:string)=>void;children:ReactNode}) {
  const [layout,setLayout]=useAccountPreference('layout',()=>{try{return readLayout(JSON.parse(localStorage.getItem('webts-layout')??'null'));}catch{return readLayout(null);}});
  const [saved,setSaved]=useState(true);
  const dragged=useRef<PanelId|null>(null),resizing=useRef<{index:number;x:number;width:number;layout:Layout}|null>(null);
  const root=useRef<HTMLElement>(null);
  const order=compact?readPanelOrder(mobileOrder):layout.order;
  useEffect(()=>{const canvas=root.current,viewport=canvas?.parentElement;if(!canvas||!viewport)return;const observer=new ResizeObserver(()=>{viewport.toggleAttribute('data-overflow',canvas.scrollWidth>viewport.clientWidth+1);});observer.observe(canvas);observer.observe(viewport);return()=>observer.disconnect();},[]);
  function update(next:Layout){setLayout(next);try{localStorage.setItem('webts-layout',JSON.stringify(next));setSaved(true);}catch{setSaved(false);}}
  const nodes=Children.toArray(children);
  return <div className="workspace-shell">{editing&&<div className="layout-toolbar">
    <span className="layout-caption">{compact?'逐页浏览 · 布局随账号保存':'保持你的布局 · 窄窗可横向滚动'}</span>
    {compact&&<button className="layout-customize" onClick={finish}>完成布局 ✓</button>}
    {editing&&<button className="text-button" onClick={()=>compact?onMobileOrder([...panelIds]):update(readLayout(defaultLayout))}>恢复默认</button>}
    {!saved&&<span role="status">布局已生效，浏览器禁止保存本机副本。</span>}
    {editing&&compact&&<div className="compact-layout-editor" aria-label="调整手机模块顺序">{order.map((id,index)=><div key={id}><strong>{labels[id]}</strong><button disabled={index===0} aria-label={`${labels[id]}前移`} onClick={()=>{const next=[...order];[next[index-1],next[index]]=[next[index],next[index-1]];onMobileOrder(next);}}>↑</button><button disabled={index===2} aria-label={`${labels[id]}后移`} onClick={()=>{const next=[...order];[next[index+1],next[index]]=[next[index],next[index+1]];onMobileOrder(next);}}>↓</button></div>)}</div>}
  </div>}<nav className="workspace-jump" aria-label="工作区模块">{order.map(id=><button key={id} aria-current={compact&&mobile===id?'page':undefined} onClick={()=>{if(compact)select(id);else root.current?.querySelector(`.pane-${id}`)?.scrollIntoView({block:'nearest',inline:'nearest',behavior:matchMedia('(prefers-reduced-motion:reduce)').matches?'instant':'smooth'});}}>{labels[id]}</button>)}</nav><div className="workspace-viewport"><main ref={root} className={`workspace mobile-${mobile} ${editing?'layout-editing':''}`} style={{gridTemplateColumns:layout.order.map(id=>`minmax(0,${layout.widths[id]}fr)`).join(' '),minWidth:compact?0:minimumWorkspaceWidth(layout)} as CSSProperties}>
    {layout.order.map((id,index)=><div key={id} className={`workspace-pane pane-${id}`} onDragOver={e=>{if(dragged.current)e.preventDefault();}} onDrop={e=>{if(!dragged.current)return;e.preventDefault();update(reorderPanels(layout,dragged.current,id));dragged.current=null;}}>
      {editing&&!compact&&<div className="layout-handle"><button draggable onDragStart={e=>{dragged.current=id;e.dataTransfer.setData('text/plain',id);e.dataTransfer.effectAllowed='move';}} onDragEnd={()=>dragged.current=null} aria-label={`拖动${labels[id]}调整位置`}>⠿ {labels[id]}</button><button disabled={index===0} aria-label={`${labels[id]}前移`} onClick={()=>update(reorderPanels(layout,id,layout.order[index-1]))}>←</button><button disabled={index===2} aria-label={`${labels[id]}后移`} onClick={()=>update(reorderPanels(layout,id,layout.order[index+1]))}>→</button></div>}
      {nodes[panelIds.indexOf(id)]}
    </div>)}
    {editing&&!compact&&[0,1].map(index=><div key={index} className="layout-separator" role="separator" aria-orientation="vertical" aria-label={`调整${labels[layout.order[index]]}宽度`} aria-valuemin={layout.order[index]==='chat'?30:15} aria-valuemax={layout.widths[layout.order[index]]+layout.widths[layout.order[index+1]]-(layout.order[index+1]==='chat'?30:15)} aria-valuenow={Math.round(layout.widths[layout.order[index]])} tabIndex={0} style={{left:`${layout.order.slice(0,index+1).reduce((sum,id)=>sum+layout.widths[id],0)}%`}} onKeyDown={e=>{if(['ArrowLeft','ArrowRight'].includes(e.key)){e.preventDefault();update(resizePanels(layout,index,e.key==='ArrowRight'?2:-2));}}} onPointerDown={e=>{if(e.button!==0||!root.current)return;e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);resizing.current={index,x:e.clientX,width:root.current.getBoundingClientRect().width,layout};}} onPointerMove={e=>{const start=resizing.current;if(start&&e.currentTarget.hasPointerCapture(e.pointerId))update(resizePanels(start.layout,start.index,(e.clientX-start.x)/start.width*100));}} onPointerUp={()=>resizing.current=null} onPointerCancel={()=>resizing.current=null} onLostPointerCapture={()=>resizing.current=null}/>) }
  </main></div></div>;
}
