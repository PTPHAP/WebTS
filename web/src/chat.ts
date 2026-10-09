export function chatVisible(message:{scope:string;from?:number;target?:number;peerUid?:string},scope:string,selected?:number,uid?:string):boolean {
  if(message.scope==='system')return scope!=='private';
  if(scope==='private')return selected!==undefined&&(uid===undefined||message.peerUid===uid)&&(message.scope==='private'||message.scope==='client')&&(message.from===selected||message.target===selected);
  return message.scope===scope;
}
