export function chatVisible(message:{scope:string;from?:number;target?:number},scope:string,selected?:number):boolean {
  if(message.scope==='system')return true;
  if(scope==='private')return selected!==undefined&&(message.scope==='private'||message.scope==='client')&&(message.from===selected||message.target===selected);
  return message.scope===scope;
}
