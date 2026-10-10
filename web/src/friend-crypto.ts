export type RatchetEngine={save:()=>string;identity:()=>string;prekeys:()=>string;has_session:(curve:string)=>boolean;outbound:(curve:string,key:string)=>void;encrypt:(curve:string,text:string)=>string;decrypt:(curve:string,packet:string)=>string;free:()=>void};
type EngineModule={default:(options?:{module_or_path?:BufferSource})=>Promise<unknown>;Engine:{new():RatchetEngine;restore:(text:string)=>RatchetEngine;verify:(ed:string,text:string,signature:string)=>boolean}};
let modulePromise:Promise<EngineModule>|undefined;
export async function cryptoModule():Promise<EngineModule>{
  modulePromise??=(async()=>{const path='/crypto/webts_crypto.js';const m=await import(/* @vite-ignore */ path) as EngineModule;await m.default();return m;})();return modulePromise;
}
export const burnDurations=[[60,'1分钟'],[600,'10分钟'],[1800,'30分钟'],[3600,'1小时'],[43200,'12小时'],[86400,'24小时'],[172800,'2天'],[345600,'4天'],[604800,'7天']] as const;
export type Envelope={version:1;site:string;id:string;sender:number;recipient:number;expires:number;text:string;image?:string;sticker?:string;burn:boolean;burn_seconds?:number};
type MessageIdentity=Pick<Envelope,'site'|'id'|'sender'|'recipient'|'burn'|'burn_seconds'>;
export type Encrypted={version:1;account:number;kind:'identity'|'state';publicKey:string;salt:string;iv:string;data:string;automatic?:boolean};
export type Cache={value:Envelope;created_at:number;mode?:'e2ee'|'server';epoch?:number;burnAt?:number};
export type Saved={engine:string;device:string;lease:string;previousLease?:string;peerDevices:Record<string,string>;cache:Record<string,Cache>;readPending?:string[];outbox?:{peer:number;id:string;ciphertext:string;burn:boolean;burn_seconds?:number;mode?:'e2ee'|'server';epoch?:number}};
export type Unlocked={engine:RatchetEngine;record:Encrypted;key:CryptoKey;saved:Saved};
export function applyReadReceipt(cache:Cache|undefined,expires:number):void{
  if(!cache?.value.burn_seconds)return;
  if(!Number.isSafeInteger(expires)||expires<=0)throw new Error('已读截止时间无效');
  cache.value.expires=Math.min(cache.value.expires,expires);
  cache.burnAt=Math.min(cache.burnAt??Infinity,expires*1000);
}
const utf8=new TextEncoder();
export function b64(bytes:Uint8Array):string{let s='';for(let i=0;i<bytes.length;i+=4096)s+=String.fromCharCode(...bytes.subarray(i,i+4096));return btoa(s);}
export function unb64(text:string):Uint8Array<ArrayBuffer>{return Uint8Array.from(atob(text),c=>c.charCodeAt(0));}
const context=(record:Pick<Encrypted,'account'|'kind'|'publicKey'>)=>utf8.encode(`webts-friend-v1:${record.account}:${record.kind}:${record.publicKey}`);
async function derive(passphrase:string,salt:Uint8Array<ArrayBuffer>):Promise<CryptoKey>{const secret=await crypto.subtle.importKey('raw',utf8.encode(passphrase),'PBKDF2',false,['deriveKey']);return crypto.subtle.deriveKey({name:'PBKDF2',hash:'SHA-256',salt,iterations:600000},secret,{name:'AES-GCM',length:256},false,['encrypt','decrypt']);}
async function protect(record:Encrypted,key:CryptoKey,text:string):Promise<Encrypted>{const iv=crypto.getRandomValues(new Uint8Array(12));const data=await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:context(record)},key,utf8.encode(text));return {...record,iv:b64(iv),data:b64(new Uint8Array(data))};}
function parseRecord(value:unknown,account:number,kind:'identity'|'state'):Encrypted{
  const r=value as Encrypted;
  if(!r||r.version!==1||r.account!==account||r.kind!==kind||r.automatic!==undefined&&typeof r.automatic!=='boolean'||typeof r.publicKey!=='string'||r.publicKey.length>256||typeof r.salt!=='string'||unb64(r.salt).length!==16||typeof r.iv!=='string'||unb64(r.iv).length!==12||typeof r.data!=='string'||r.data.length>16*1024*1024)throw new Error('备份格式、账号或容量不匹配');return r;
}
async function unprotect(record:Encrypted,key:CryptoKey):Promise<string>{const value=await crypto.subtle.decrypt({name:'AES-GCM',iv:unb64(record.iv),additionalData:context(record)},key,unb64(record.data));return new TextDecoder('utf-8',{fatal:true}).decode(value);}
async function deviceKey(account:number,create=false):Promise<CryptoKey>{
  let key=await loadLocal<CryptoKey>('keys',`device:${account}`);
  if(!key&&create){key=await crypto.subtle.generateKey({name:'AES-GCM',length:256},false,['encrypt','decrypt']);await saveLocal('keys',`device:${account}`,key);}
  if(!key||key.type!=='secret'||key.extractable||key.algorithm.name!=='AES-GCM'||(key.algorithm as AesKeyAlgorithm).length!==256||!key.usages.includes('encrypt')||!key.usages.includes('decrypt'))throw new Error('此浏览器的自动密钥不可用，请导入受口令保护的身份备份。');return key;
}
export async function createAutomaticIdentity(account:number):Promise<Encrypted>{
  const key=await deviceKey(account,true),m=await cryptoModule(),engine=new m.Engine();
  try{const record:Encrypted={version:1,account,kind:'identity',publicKey:engine.identity(),salt:b64(crypto.getRandomValues(new Uint8Array(16))),iv:'',data:'',automatic:true};return await protect(record,key,engine.save());}finally{engine.free();}
}
export async function exportIdentity(identity:Encrypted,passphrase:string):Promise<Encrypted>{
  if(passphrase.length<16||passphrase.length>256)throw new Error('备份口令需要16–256个字符。');
  const raw=await unprotect(identity,identity.automatic?await deviceKey(identity.account):await derive(passphrase,unb64(identity.salt)));
  const salt=crypto.getRandomValues(new Uint8Array(16));return protect({...identity,automatic:undefined,salt:b64(salt)},await derive(passphrase,salt),raw);
}
export async function saveImportedIdentity(identity:Encrypted):Promise<void>{
  parseRecord(identity,identity.account,'identity');if(identity.automatic)throw new Error('只能导入口令保护的可移植备份');
  await saveLocals([[`identity:${identity.account}`,identity],[`state:${identity.account}`,undefined]]);
}
export async function rememberDevice(identity:Encrypted,state:Unlocked,passphrase:string):Promise<Encrypted>{
  if(identity.automatic)return identity;
  const raw=await unprotect(identity,await derive(passphrase,unb64(identity.salt))),key=await deviceKey(identity.account,true);
  const savedIdentity=await protect({...identity,automatic:true},key,raw);
  state.saved.engine=state.engine.save();const record=await protect({...state.record,automatic:true},key,JSON.stringify(state.saved));
  await saveLocals([[`identity:${identity.account}`,savedIdentity],[`state:${identity.account}`,record]]);state.key=key;state.record=record;return savedIdentity;
}
export async function createIdentity(account:number,passphrase:string):Promise<Encrypted>{
  if(passphrase.length<16||passphrase.length>256)throw new Error('私信密钥口令需要16–256个字符，建议使用独立的长句。');
  const m=await cryptoModule(),engine=new m.Engine();try{const salt=crypto.getRandomValues(new Uint8Array(16));const record:Encrypted={version:1,account,kind:'identity',publicKey:engine.identity(),salt:b64(salt),iv:'',data:''};return await protect(record,await derive(passphrase,salt),engine.save());}finally{engine.free();}
}
export async function unlockState(identity:Encrypted,local:Encrypted|undefined,account:number,publicKey:string,passphrase:string):Promise<Unlocked>{
  const source=parseRecord(local??identity,account,local?'state':'identity');
  if(source.publicKey!==publicKey||identity.publicKey!==publicKey)throw new Error('备份与账号已登记的私信身份不一致');
  const m=await cryptoModule(),key=source.automatic?await deviceKey(account):await derive(passphrase,unb64(source.salt));const raw=await unprotect(source,key);
  const saved:Saved=local?JSON.parse(raw):{engine:raw,device:crypto.randomUUID(),lease:Array.from(crypto.getRandomValues(new Uint8Array(32)),b=>b.toString(16).padStart(2,'0')).join(''),peerDevices:{},cache:{}};
  const engine=m.Engine.restore(saved.engine);
  if(engine.identity()!==publicKey){engine.free();throw new Error('私信身份校验失败');}
  return {engine,key,saved,record:{...source,kind:'state'}};
}
export async function persist(state:Unlocked):Promise<void>{
  state.saved.engine=state.engine.save();const now=Date.now()/1000;
  const entries=Object.entries(state.saved.cache).filter(([,v])=>v.mode!=='server'&&(!v.value.burn||v.value.burn_seconds!==undefined)&&v.value.expires>now&&(!v.burnAt||v.burnAt>now*1000)).sort((a,b)=>a[1].created_at-b[1].created_at).slice(-60);
  let bytes=0;
  state.saved.cache=Object.fromEntries(entries.reverse().filter(([,v])=>{const size=utf8.encode(JSON.stringify(v)).byteLength;if(bytes+size>2*1024*1024)return false;bytes+=size;return true;}));
  state.record=await protect(state.record,state.key,JSON.stringify(state.saved));await saveLocal('keys',`state:${state.record.account}`,state.record);
}
export async function safetyCode(own:string,peer:string):Promise<string>{const pair=[own,peer].sort();const bytes=await crypto.subtle.digest('SHA-256',utf8.encode(`webts-olm-safety-v1:${pair.join('\n')}`));return Array.from(new Uint8Array(bytes),b=>b.toString(16).padStart(2,'0')).join('').match(/.{1,8}/g)!.join(' ');}
export async function checkPrekey(identity:string,claimed:{key:string;bundle:{payload:string;signature:string;device:string}}):Promise<{curve:string;device:string}>{
  const m=await cryptoModule(),root=JSON.parse(identity),payload=JSON.parse(claimed.bundle.payload);
  if(payload.identity!==identity||!Array.isArray(payload.keys)||!payload.keys.includes(claimed.key)||!m.Engine.verify(root.ed,claimed.bundle.payload,claimed.bundle.signature))throw new Error('对方一次性公钥签名或身份不匹配，已停止发送');return {curve:root.curve,device:claimed.bundle.device};
}
export function validEnvelope(value:unknown,expected:MessageIdentity):Envelope{
  const v=value as Envelope;
  if(v?.burn_seconds!==expected.burn_seconds||v?.burn_seconds!==undefined&&(!v.burn||!burnDurations.some(([seconds])=>seconds===v.burn_seconds)))throw new Error('阅后即焚时长不匹配');
  if(!v||v.version!==1||v.site!==expected.site||v.id!==expected.id||v.sender!==expected.sender||v.recipient!==expected.recipient||v.burn!==expected.burn||!Number.isSafeInteger(v.expires)||v.expires>Date.now()/1000+31*86400||v.expires<=0||typeof v.text!=='string'||v.text.length>4096||v.image!==undefined&&(typeof v.image!=='string'||v.image.length>170000||!/^data:image\/jpeg;base64,[A-Za-z0-9+/]+={0,2}$/.test(v.image))||v.sticker!==undefined&&(typeof v.sticker!=='string'||v.sticker.length>32))throw new Error('消息身份、有效期或图片格式不匹配');return v;
}
const messageContext=(v:Pick<Envelope,'site'|'id'|'sender'|'recipient'|'burn'>)=>utf8.encode(`webts-olm-content-v1:${v.site}:${v.id}:${v.sender}:${v.recipient}:${v.burn}`);
export async function sealMessage(value:Envelope,state:Unlocked,curve:string,device:string):Promise<string>{
  validEnvelope(value,value);const keyBytes=crypto.getRandomValues(new Uint8Array(32)),iv=crypto.getRandomValues(new Uint8Array(12));
  const key=await crypto.subtle.importKey('raw',keyBytes,'AES-GCM',false,['encrypt']);
  const data=await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:messageContext(value)},key,utf8.encode(JSON.stringify(value)));
  const secret={version:1,site:value.site,id:value.id,sender:value.sender,recipient:value.recipient,burn:value.burn,key:b64(keyBytes),iv:b64(iv),device};keyBytes.fill(0);
  const packet=state.engine.encrypt(curve,JSON.stringify(secret));return JSON.stringify({version:1,device,packet,iv:b64(iv),content:b64(new Uint8Array(data))});
}
export async function openMessage(ciphertext:string,state:Unlocked,curve:string,expected:MessageIdentity):Promise<Envelope>{
  if(ciphertext.length>384*1024)throw new Error('密文超过安全上限');const outer=JSON.parse(ciphertext);
  if(outer.version!==1||outer.device!==state.saved.device||typeof outer.packet!=='string')throw new Error('消息属于之前的私信设备，当前设备无法读取');
  const secret=JSON.parse(state.engine.decrypt(curve,outer.packet));
  if(secret.version!==1||secret.site!==expected.site||secret.id!==expected.id||secret.sender!==expected.sender||secret.recipient!==expected.recipient||secret.burn!==expected.burn||secret.device!==outer.device||secret.iv!==outer.iv)throw new Error('棘轮消息上下文不匹配');
  const raw=unb64(secret.key);if(raw.length!==32)throw new Error('消息密钥无效');
  const key=await crypto.subtle.importKey('raw',raw,'AES-GCM',false,['decrypt']);raw.fill(0);
  const data=await crypto.subtle.decrypt({name:'AES-GCM',iv:unb64(outer.iv),additionalData:messageContext(expected)},key,unb64(outer.content));
  if(data.byteLength>192*1024)throw new Error('正文超过安全上限');return validEnvelope(JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(data)),expected);
}
const serverContext=(v:Pick<Envelope,'site'|'id'|'sender'|'recipient'|'burn'>,epoch:number)=>utf8.encode(`webts-server-content-v1:${v.site}:${v.id}:${v.sender}:${v.recipient}:${v.burn}:${epoch}`);
export async function sealServerMessage(value:Envelope,epoch:number):Promise<string>{
  validEnvelope(value,value);if(!Number.isSafeInteger(epoch)||epoch<0)throw new Error('发送方式版本无效。');
  const bytes=crypto.getRandomValues(new Uint8Array(32)),iv=crypto.getRandomValues(new Uint8Array(12));
  const key=await crypto.subtle.importKey('raw',bytes,'AES-GCM',false,['encrypt']);
  const data=await crypto.subtle.encrypt({name:'AES-GCM',iv,additionalData:serverContext(value,epoch)},key,utf8.encode(JSON.stringify(value)));
  const encoded=b64(bytes);bytes.fill(0);
  // This key deliberately reaches the site over HTTPS. This mode is not E2EE.
  return JSON.stringify({version:1,mode:'server',epoch,key:encoded,iv:b64(iv),content:b64(new Uint8Array(data))});
}
export async function openServerMessage(ciphertext:string,epoch:number,expected:MessageIdentity):Promise<Envelope>{
  if(ciphertext.length>384*1024)throw new Error('密文超过安全上限');const outer=JSON.parse(ciphertext);
  if(!Number.isSafeInteger(epoch)||epoch<0||outer.version!==1||outer.mode!=='server'||outer.epoch!==epoch||typeof outer.key!=='string'||typeof outer.iv!=='string'||typeof outer.content!=='string')throw new Error('服务器可解密消息的模式不匹配');
  const raw=unb64(outer.key),iv=unb64(outer.iv);if(raw.length!==32||iv.length!==12)throw new Error('必须使用AES-256-GCM');
  const key=await crypto.subtle.importKey('raw',raw,'AES-GCM',false,['decrypt']);raw.fill(0);
  const data=await crypto.subtle.decrypt({name:'AES-GCM',iv,additionalData:serverContext(expected,epoch)},key,unb64(outer.content));
  if(data.byteLength>192*1024)throw new Error('正文超过安全上限');return validEnvelope(JSON.parse(new TextDecoder('utf-8',{fatal:true}).decode(data)),expected);
}
function database():Promise<IDBDatabase>{return new Promise((resolve,reject)=>{const r=indexedDB.open('webts-private-messaging',1);r.onupgradeneeded=()=>{r.result.createObjectStore('keys');r.result.createObjectStore('trust');};r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(new Error('浏览器无法保存加密密钥'));});}
export async function loadLocal<T>(store:'keys'|'trust',key:string):Promise<T|undefined>{const db=await database();try{return await new Promise<T|undefined>((resolve,reject)=>{const tx=db.transaction(store);const r=tx.objectStore(store).get(key);r.onsuccess=()=>resolve(r.result);r.onerror=()=>reject(new Error('本地记录读取失败'));});}finally{db.close();}}
async function saveLocals(entries:[string,unknown][],store:'keys'|'trust'='keys'):Promise<void>{const db=await database();try{await new Promise<void>((resolve,reject)=>{const tx=db.transaction(store,'readwrite');for(const [key,value] of entries){const object=tx.objectStore(store);if(value===undefined)object.delete(key);else object.put(value,key);}tx.oncomplete=()=>resolve();tx.onerror=()=>reject(new Error('本地记录保存失败'));tx.onabort=()=>reject(new Error('本地记录保存取消'));});}finally{db.close();}}
export async function saveLocal(store:'keys'|'trust',key:string,value:unknown):Promise<void>{await saveLocals([[key,value]],store);}
