import {loadLocal,saveLocal} from './friend-crypto';
import type {MoveTarget} from './channel-moves';
type Stored={iv:Uint8Array<ArrayBuffer>;data:ArrayBuffer;expires:number};
async function key(context:string):Promise<CryptoKey>{const name=`channel-key:${context}`;let value=await loadLocal<CryptoKey>('keys',name);if(!value){if(!navigator.locks)throw new Error('浏览器不支持安全保存频道密码');value=await navigator.locks.request(name,async()=>{const existing=await loadLocal<CryptoKey>('keys',name);if(existing)return existing;const created=await crypto.subtle.generateKey({name:'AES-GCM',length:256},false,['encrypt','decrypt']);await saveLocal('keys',name,created);return created;});}return value;}
function address(target:MoveTarget){if(!target.context||!Number.isSafeInteger(target.channel)||target.channel<1)throw new Error('频道密码上下文无效');return `channel-password:${target.context}:${target.channel}`;}
export const channelPasswords={
  async load(target:MoveTarget):Promise<string|undefined>{const name=address(target),stored=await loadLocal<Stored>('keys',name);if(!stored)return;if(stored.expires<=Date.now()){await saveLocal('keys',name,undefined);return;}try{return new TextDecoder('utf-8',{fatal:true}).decode(await crypto.subtle.decrypt({name:'AES-GCM',iv:stored.iv,additionalData:new TextEncoder().encode(name)},await key(target.context!),stored.data));}catch{await saveLocal('keys',name,undefined);return;}},
  async save(target:MoveTarget,password:string){const name=address(target),iv=crypto.getRandomValues(new Uint8Array(12)),data=await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:new TextEncoder().encode(name)},await key(target.context!),new TextEncoder().encode(password));await saveLocal('keys',name,{iv,data,expires:Date.now()+30*86400000});},
  async forget(target:MoveTarget){await saveLocal('keys',address(target),undefined);}
};
