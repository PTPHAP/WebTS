import {useEffect,useRef} from 'react';
import type {ReactNode} from 'react';
export function ContextMenu({x,y,close,children,label}:{x:number;y:number;close:()=>void;children:ReactNode;label:string}){
  const ref=useRef<HTMLDivElement>(null);
  useEffect(()=>{const old=document.activeElement as HTMLElement|null;ref.current?.querySelector<HTMLButtonElement>('button:not(:disabled)')?.focus();const outside=(e:PointerEvent)=>{if(!ref.current?.contains(e.target as Node))close();};const escape=(e:KeyboardEvent)=>{if(e.key==='Escape'){e.preventDefault();close();}};document.addEventListener('pointerdown',outside);document.addEventListener('keydown',escape);window.addEventListener('resize',close);return()=>{document.removeEventListener('pointerdown',outside);document.removeEventListener('keydown',escape);window.removeEventListener('resize',close);old?.focus();};},[close]);
  const top=Math.max(8,Math.min(y,innerHeight-300));
  return <div ref={ref} role="menu" aria-label={label} className="context-menu" style={{left:Math.max(8,Math.min(x,innerWidth-260)),top,maxHeight:Math.max(80,innerHeight-top-8)}} onKeyDown={e=>{if(!['ArrowDown','ArrowUp','Home','End','Tab'].includes(e.key))return;e.preventDefault();if(e.key==='Tab'){close();return;}const items=[...e.currentTarget.querySelectorAll<HTMLButtonElement>('button:not(:disabled)')],index=items.indexOf(document.activeElement as HTMLButtonElement);items[e.key==='Home'?0:e.key==='End'?items.length-1:(index+(e.key==='ArrowUp'?-1:1)+items.length)%items.length]?.focus();}}><strong>{label}</strong>{children}</div>;
}
