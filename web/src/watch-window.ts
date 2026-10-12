export type WatchPoint={x:number;y:number};
export type WatchBounds={left:number;top:number;right:number;bottom:number};
export type WatchRect={x:number;y:number;width:number;height:number};

// Positions are page-local, never shared with other devices or accounts.
export function miniWatchRect(bounds:WatchBounds,point:WatchPoint|null,folded:boolean,minimumVideo?:{width:number;height:number}):WatchRect&{collapsed:boolean}{
  const margin=8,availableWidth=Math.max(0,bounds.right-bounds.left-margin*2),availableHeight=Math.max(0,bounds.bottom-bounds.top-margin*2);
  const collapsed=folded||availableHeight<182||!!minimumVideo&&(availableWidth<minimumVideo.width||availableHeight<minimumVideo.height+92);
  const preferred=minimumVideo?Math.max(420,minimumVideo.width):availableWidth<680?280:360;
  const width=Math.min(availableWidth,preferred,collapsed||minimumVideo?preferred:Math.max(0,(availableHeight-92)*16/9));
  const height=Math.min(availableHeight,collapsed?44:92+Math.max(width*9/16,minimumVideo?.height??0));
  const left=bounds.left+margin,top=bounds.top+margin;
  const clamp=(value:number,min:number,max:number)=>Math.max(min,Math.min(value,max));
  return {x:clamp(point?.x??bounds.right-width-20,left,bounds.right-width-margin),y:clamp(point?.y??bounds.bottom-height-20,top,bounds.bottom-height-margin),width,height,collapsed};
}

export function watchShouldMount(visited:boolean,view:string){return visited||view==='watch';}
