export type MoveTarget={channel:number;name:string;client?:number;context?:string};
type Passwords={load:(target:MoveTarget)=>Promise<string|undefined>;save:(target:MoveTarget,password:string)=>Promise<void>;forget:(target:MoveTarget)=>Promise<void>};
type Result={id?:unknown;ok?:unknown;code?:unknown;message?:unknown};
// Only the TS verdict can decide whether this identity needs a channel password.
export class ChannelMoves {
  private pending?:{id:string;target:MoveTarget;password:string;remember:boolean;cached:boolean};
  private waiting?:MoveTarget;
  private generation=0;
  constructor(private send:(value:Record<string,unknown>)=>void,private ask:(target:MoveTarget)=>void,private done:(target:MoveTarget)=>void,private notice:(message:string)=>void,private passwords?:Passwords){}
  start(target:MoveTarget) {
    if(this.pending||this.waiting){this.notice('请先完成或取消当前频道切换。');return;}
    this.submit(target,'');
  }
  private submit(target:MoveTarget,password:string,remember=false,cached=false){const id=crypto.randomUUID();this.pending={id,target,password,remember,cached};this.send({type:'command',id,action:'move',channel:target.channel,...(target.client===undefined?{}:{client:target.client}),password});}
  password(value:string,remember=false){if(!this.waiting)return;const target=this.waiting;this.waiting=undefined;this.submit(target,value,remember);}
  result(value:Result):boolean {
    if(!this.pending||value.id!==this.pending.id)return false;
    const {target,password,remember,cached}=this.pending;this.pending=undefined;
    if(value.ok===true){if(this.passwords&&target.context&&target.client===undefined&&remember)void this.passwords.save(target,password).catch(()=>this.notice('已进入频道，但本机密码保存失败。'));this.done(target);}
    else if(value.code===781){this.waiting=target;const generation=this.generation;if(this.passwords&&target.context&&target.client===undefined&&!password&&!cached){void this.passwords.load(target).then(saved=>{if(this.generation!==generation||this.waiting!==target)return;if(saved){this.waiting=undefined;this.submit(target,saved,false,true);}else this.ask(target);}).catch(()=>{if(this.generation===generation&&this.waiting===target)this.ask(target);});}else{if(this.passwords&&target.context&&cached){void this.passwords.forget(target).catch(()=>this.notice('本机旧频道密码清除失败。')).then(()=>{if(this.generation===generation&&this.waiting===target)this.ask(target);});}else this.ask(target);}}
    else this.notice(typeof value.message==='string'?value.message:'频道切换被服务器拒绝');
    return true;
  }
  reset(){this.generation++;this.pending=undefined;this.waiting=undefined;}
}
