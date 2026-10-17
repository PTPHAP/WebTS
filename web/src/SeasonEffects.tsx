import {memo,useEffect,useRef} from 'react';
import {readSeason} from './seasons';

export const SeasonEffects=memo(function SeasonEffects({value}:{value:unknown}){
  const layer=useRef<HTMLDivElement>(null),season=readSeason(value);
  useEffect(()=>{
    if(season==='classic'||!layer.current)return;
    const container=layer.current,reduced=matchMedia('(prefers-reduced-motion: reduce)'),coarse=matchMedia('(pointer: coarse)').matches;
    const bursts=new Map<HTMLElement,ReturnType<typeof setTimeout>>();
    let gesture:{id:number;x:number;y:number;time:number}|undefined,last=-Infinity,sequence=0;
    function remove(node:HTMLElement){clearTimeout(bursts.get(node));bursts.delete(node);node.remove();}
    function clear(){gesture=undefined;for(const node of bursts.keys())remove(node);}
    function visibility(){document.documentElement.toggleAttribute('data-season-paused',document.hidden);if(document.hidden)clear();}
    function preference(){if(reduced.matches)clear();}
    function cancel(){gesture=undefined;}
    function down(e:PointerEvent){gesture=e.isPrimary&&e.button===0?{id:e.pointerId,x:e.clientX,y:e.clientY,time:performance.now()}:undefined;}
    function up(e:PointerEvent){
      const start=gesture;gesture=undefined;
      if(!start||e.pointerId!==start.id||!e.isPrimary||e.button!==0||reduced.matches||document.hidden)return;
      const now=performance.now();
      if(now-start.time>800||now-last<120||Math.hypot(e.clientX-start.x,e.clientY-start.y)>10)return;
      if(!(e.target instanceof Element)||e.target.closest('input,textarea,select,option,[contenteditable],dialog,canvas,video,audio,iframe,[data-no-season-effects]'))return;
      last=now;
      while(bursts.size>=(coarse?2:3))remove(bursts.keys().next().value!);
      const node=document.createElement('div');node.className='season-click-burst';node.style.left=`${Math.round(e.clientX)}px`;node.style.top=`${Math.round(e.clientY)}px`;
      const ring=document.createElement('i');ring.className='season-click-ring';node.append(ring);
      const count=coarse?5:8,turn=sequence++*.65;
      for(let i=0;i<count;i++){
        const part=document.createElement('i'),angle=2*Math.PI*i/count+turn,distance=(coarse?30:44)+(i%3)*6;
        part.className='season-click-particle';part.style.setProperty('--dx',`${Math.cos(angle)*distance}px`);part.style.setProperty('--dy',`${Math.sin(angle)*distance}px`);part.style.setProperty('--spin',`${90+i*49}deg`);node.append(part);
      }
      node.addEventListener('animationend',e=>{if(e.target===node)remove(node);});
      bursts.set(node,setTimeout(()=>remove(node),1100));container.append(node);
    }
    visibility();
    document.addEventListener('pointerdown',down,{capture:true,passive:true});document.addEventListener('pointerup',up,{capture:true,passive:true});document.addEventListener('pointercancel',cancel,true);document.addEventListener('scroll',cancel,{capture:true,passive:true});document.addEventListener('visibilitychange',visibility);reduced.addEventListener('change',preference);
    return()=>{document.removeEventListener('pointerdown',down,true);document.removeEventListener('pointerup',up,true);document.removeEventListener('pointercancel',cancel,true);document.removeEventListener('scroll',cancel,true);document.removeEventListener('visibilitychange',visibility);reduced.removeEventListener('change',preference);clear();document.documentElement.removeAttribute('data-season-paused');};
  },[season]);
  return season==='classic'?null:<div className="season-click-layer" data-festival={season} ref={layer} aria-hidden="true"/>;
});
