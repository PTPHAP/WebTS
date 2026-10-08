export type MoveTarget={channel:number;name:string;client?:number};
type Result={id?:unknown;ok?:unknown;code?:unknown;message?:unknown};
// Only the TS verdict can decide whether this identity needs a channel password.
export class ChannelMoves {
  private pending?:{id:string;target:MoveTarget};
  private waiting?:MoveTarget;
  constructor(private send:(value:Record<string,unknown>)=>void,private ask:(target:MoveTarget)=>void,private done:(target:MoveTarget)=>void,private notice:(message:string)=>void){}
  start(target:MoveTarget) {
    if(this.pending||this.waiting){this.notice('请先完成或取消当前频道切换。');return;}
    this.submit(target,'');
  }
  private submit(target:MoveTarget,password:string){const id=crypto.randomUUID();this.pending={id,target};this.send({type:'command',id,action:'move',channel:target.channel,...(target.client===undefined?{}:{client:target.client}),password});}
  password(value:string){if(!this.waiting)return;const target=this.waiting;this.waiting=undefined;this.submit(target,value);}
  result(value:Result):boolean {
    if(!this.pending||value.id!==this.pending.id)return false;
    const {target}=this.pending;this.pending=undefined;
    if(value.ok===true)this.done(target);
    else if(value.code===781){this.waiting=target;this.ask(target);}
    else this.notice(typeof value.message==='string'?value.message:'频道切换被服务器拒绝');
    return true;
  }
  reset(){this.pending=undefined;this.waiting=undefined;}
}
