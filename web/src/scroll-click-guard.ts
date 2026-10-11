import type {MouseEvent,PointerEvent,UIEvent} from 'react';

// Scrolling must remain native. Only suppress the click after a moved/cancelled gesture.
export function createScrollClickGuard(){
  let gesture:{id:number;x:number;y:number;moved:boolean}|undefined;
  let blockedUntil=0;
  const block=()=>{blockedUntil=performance.now()+400;};
  return {
    onPointerDownCapture(e:PointerEvent<HTMLElement>){
      if(e.isPrimary===false){if(gesture)gesture.moved=true;block();return;}
      if(e.button!==0)return;blockedUntil=0;gesture={id:e.pointerId,x:e.clientX,y:e.clientY,moved:false};
    },
    onPointerMoveCapture(e:PointerEvent<HTMLElement>){
      if(gesture?.id===e.pointerId&&Math.hypot(e.clientX-gesture.x,e.clientY-gesture.y)>10)gesture.moved=true;
    },
    onScrollCapture(_e:UIEvent<HTMLElement>){if(gesture)gesture.moved=true;},
    onPointerUpCapture(e:PointerEvent<HTMLElement>){if(gesture?.id!==e.pointerId)return;if(gesture.moved)block();gesture=undefined;},
    onPointerCancelCapture(e:PointerEvent<HTMLElement>){if(gesture?.id===e.pointerId){block();gesture=undefined;}},
    onClickCapture(e:MouseEvent<HTMLElement>){if(e.detail>0&&performance.now()<blockedUntil){e.preventDefault();e.stopPropagation();blockedUntil=0;}},
    onContextMenuCapture(e:MouseEvent<HTMLElement>){if(gesture?.moved||performance.now()<blockedUntil){e.preventDefault();e.stopPropagation();}},
  };
}
