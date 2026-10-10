import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
import vm from 'node:vm';
globalThis.__webtsTestWasm=new Uint8Array(await readFile(new URL('../public/crypto/webts_crypto_bg.wasm',import.meta.url)));
const binding=new URL('../public/crypto/webts_crypto.js',import.meta.url).href;
const source=(await readFile(new URL('../src/friend-crypto.ts',import.meta.url),'utf8')).replace("const path='/crypto/webts_crypto.js'",`const path=${JSON.stringify(binding)}`).replace('await m.default();','await m.default({module_or_path:globalThis.__webtsTestWasm});');
const code=ts.transpileModule(source,{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const c=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
const m=await c.cryptoModule();
const phrase='a separate long fixture passphrase';
const local=new Map();
let failCommit=false;
// Minimal asynchronous IndexedDB fixture; cryptography uses the real generated WASM.
globalThis.indexedDB={open(){const request={};queueMicrotask(()=>{request.result={close(){},transaction(store){const pending=new Map();let scheduled=false;const tx={objectStore(){return {put(value,key){pending.set(`${store}:${key}`,structuredClone(value));if(!scheduled){scheduled=true;queueMicrotask(()=>{if(failCommit){failCommit=false;tx.onabort?.();return;}for(const [k,v] of pending){if(v===undefined)local.delete(k);else local.set(k,v);}tx.oncomplete?.();});}},delete(key){this.put(undefined,key);},get(key){const r={};queueMicrotask(()=>{r.result=structuredClone(local.get(`${store}:${key}`));r.onsuccess?.();});return r;}};}};return tx;}};request.onsuccess?.();});return request;}};
async function state(id){const backup=await c.createIdentity(id,phrase);const value=await c.unlockState(backup,undefined,id,backup.publicKey,phrase);return {backup,value,root:JSON.parse(backup.publicKey)};}
function envelope(id=crypto.randomUUID(),burn=false){return {version:1,site:'https://fixture.example',id,sender:1,recipient:2,expires:Math.floor(Date.now()/1000)+86400,text:'你好 hello 🎧',image:'data:image/jpeg;base64,c2FmZQ==',sticker:'hello',burn};}
const expected=v=>({site:v.site,id:v.id,sender:v.sender,recipient:v.recipient,burn:v.burn});
test('automatic non-extractable AES-256 device keys restore encrypted state without an extra passphrase',async()=>{
  const identity=await c.createAutomaticIdentity(11);await c.saveLocal('keys','identity:11',identity);
  const deviceKey=await c.loadLocal('keys','device:11');assert.equal(deviceKey.extractable,false);assert.equal(deviceKey.algorithm.length,256);await assert.rejects(crypto.subtle.exportKey('raw',deviceKey));
  const state=await c.unlockState(identity,undefined,11,identity.publicKey,'');state.saved.previousLease='a'.repeat(64);await c.persist(state);
  const record=await c.loadLocal('keys','state:11');assert.doesNotMatch(JSON.stringify(record),/previousLease|peerDevices|sessions/);
  const restored=await c.unlockState(identity,record,11,identity.publicKey,'');assert.equal(restored.saved.device,state.saved.device);assert.equal(restored.saved.previousLease,'a'.repeat(64));
  const portable=await c.exportIdentity(identity,phrase);assert.equal(portable.automatic,undefined);await assert.rejects(c.unlockState(portable,undefined,11,identity.publicKey,'wrong backup passphrase'));
  const imported=await c.unlockState(portable,undefined,11,identity.publicKey,phrase);assert.equal(imported.engine.identity(),identity.publicKey);assert.notEqual(imported.saved.device,state.saved.device);
  local.delete('keys:device:11');await assert.rejects(c.unlockState(identity,record,11,identity.publicKey,''),/自动密钥不可用/);assert.deepEqual(await c.loadLocal('keys','state:11'),record);
  await c.saveLocal('keys','device:11',await crypto.subtle.generateKey({name:'AES-GCM',length:256},false,['encrypt','decrypt']));await assert.rejects(c.unlockState(identity,record,11,identity.publicKey,''));
  await assert.rejects(c.unlockState(identity,record,12,identity.publicKey,''));restored.engine.free();imported.engine.free();state.engine.free();
});
test('remembering a legacy device atomically replaces both encrypted records and preserves ratchets',async()=>{
  const old=await c.createIdentity(12,phrase),state=await c.unlockState(old,undefined,12,old.publicKey,phrase);await c.saveLocal('keys','identity:12',old);await c.persist(state);const before=await c.loadLocal('keys','state:12');
  // Generate the key before faulting the transaction that contains both records.
  const key=await crypto.subtle.generateKey({name:'AES-GCM',length:256},false,['encrypt','decrypt']);await c.saveLocal('keys','device:12',key);failCommit=true;
  await assert.rejects(c.rememberDevice(old,state,phrase),/保存取消/);assert.deepEqual(await c.loadLocal('keys','identity:12'),old);assert.deepEqual(await c.loadLocal('keys','state:12'),before);assert.notEqual(state.key,key);
  const auto=await c.rememberDevice(old,state,phrase);assert.equal(auto.automatic,true);const restored=await c.unlockState(auto,await c.loadLocal('keys','state:12'),12,auto.publicKey,'');assert.equal(restored.saved.lease,state.saved.lease);assert.equal(restored.engine.save(),state.engine.save());restored.engine.free();state.engine.free();
});
test('a valid portable backup can recover a missing device key without leaving undecryptable old state',async()=>{
  const identity=await c.createAutomaticIdentity(14),state=await c.unlockState(identity,undefined,14,identity.publicKey,'');await c.saveLocal('keys','identity:14',identity);await c.persist(state);const oldState=await c.loadLocal('keys','state:14'),portable=await c.exportIdentity(identity,phrase);
  local.delete('keys:device:14');await assert.rejects(c.unlockState(identity,oldState,14,identity.publicKey,''));failCommit=true;await assert.rejects(c.saveImportedIdentity(portable));assert.deepEqual(await c.loadLocal('keys','state:14'),oldState);assert.deepEqual(await c.loadLocal('keys','identity:14'),identity);
  await c.saveImportedIdentity(portable);assert.equal(await c.loadLocal('keys','state:14'),undefined);const restored=await c.unlockState(await c.loadLocal('keys','identity:14'),undefined,14,identity.publicKey,phrase);assert.notEqual(restored.saved.device,state.saved.device);await assert.rejects(c.saveImportedIdentity(identity));restored.engine.free();state.engine.free();
});
test('server-readable messages use AES-256-GCM and bind origin, participants, burn and mode epoch',async()=>{
  const value=envelope(),cipher=await c.sealServerMessage(value,2);assert.doesNotMatch(cipher,/hello|你好|data:image/);assert.equal(c.unb64(JSON.parse(cipher).key).length,32);assert.deepEqual(await c.openServerMessage(cipher,2,expected(value)),value);
  for(const wrong of [{site:'https://attacker.example'},{id:crypto.randomUUID()},{sender:3},{recipient:4},{burn:true}])await assert.rejects(c.openServerMessage(cipher,2,{...expected(value),...wrong}));
  await assert.rejects(c.openServerMessage(cipher,3,expected(value)));await assert.rejects(c.sealServerMessage(value,0));
  const packet=JSON.parse(cipher);for(const wrong of [{key:c.b64(new Uint8Array(16))},{mode:'e2ee'},{iv:c.b64(new Uint8Array(11))},{content:c.b64(new Uint8Array(32))}])await assert.rejects(c.openServerMessage(JSON.stringify({...packet,...wrong}),2,expected(value)));
});
test('browser and Rust share an authenticated standard AES-256-GCM wire fixture',async()=>{
  const f=JSON.parse(await readFile(new URL('../../server/tests/fixtures/friend-server-content.json',import.meta.url),'utf8')),original=Date.now;
  Date.now=()=>f.clock*1000;
  try{assert.deepEqual(await c.openServerMessage(JSON.stringify(f.packet),f.epoch,expected(f.envelope)),f.envelope);}finally{Date.now=original;}
});
async function pair(){const a=await state(1),b=await state(2);const pre=JSON.parse(b.value.engine.prekeys()),payload=JSON.parse(pre.payload);const claimed={key:payload.keys[0],bundle:{...pre,device:b.value.saved.device}};await c.checkPrekey(b.backup.publicKey,claimed);a.value.engine.outbound(b.root.curve,claimed.key);return {a,b,claimed};}
test('backups contain only authenticated encrypted state and require the original account and passphrase',async()=>{
  const a=await state(1);assert.doesNotMatch(JSON.stringify(a.backup),/private_key|sessions|lease/);await assert.rejects(c.unlockState(a.backup,undefined,1,a.backup.publicKey,'a wrong fixture password'));await assert.rejects(c.unlockState(a.backup,undefined,2,a.backup.publicKey,phrase));
  await c.persist(a.value);const saved=await c.loadLocal('keys','state:1');assert.doesNotMatch(JSON.stringify(saved),/lease|peerDevices|sessions/);const restored=await c.unlockState(a.backup,saved,1,a.backup.publicKey,phrase);assert.equal(restored.saved.lease,a.value.saved.lease);assert.equal(restored.engine.identity(),a.backup.publicKey);restored.engine.free();a.value.engine.free();
});
test('signed one-time keys cannot be substituted or assigned to another identity',async()=>{
  const {a,b,claimed}=await pair();await assert.rejects(c.checkPrekey(a.backup.publicKey,claimed));await assert.rejects(c.checkPrekey(b.backup.publicKey,{...claimed,key:'forged'}));await assert.rejects(c.checkPrekey(b.backup.publicKey,{...claimed,bundle:{...claimed.bundle,payload:claimed.bundle.payload+' '}}));assert.equal(await c.safetyCode(a.backup.publicKey,b.backup.publicKey),await c.safetyCode(b.backup.publicKey,a.backup.publicKey));a.value.engine.free();b.value.engine.free();
});
test('real WASM ratchet carries encrypted text, images and stickers bidirectionally after restoration',async()=>{
  const {a,b}=await pair(),value=envelope();const cipher=await c.sealMessage(value,a.value,b.root.curve,b.value.saved.device);assert.doesNotMatch(cipher,/hello|你好|data:image/);assert.deepEqual(await c.openMessage(cipher,b.value,a.root.curve,expected(value)),value);
  const reply={...envelope(),sender:2,recipient:1,text:'收到'};const back=await c.sealMessage(reply,b.value,a.root.curve,a.value.saved.device);assert.deepEqual(await c.openMessage(back,a.value,b.root.curve,expected(reply)),reply);
  await c.persist(b.value);const restored=await c.unlockState(b.backup,await c.loadLocal('keys','state:2'),2,b.backup.publicKey,phrase);const one=envelope(),two=envelope();const first=await c.sealMessage(one,a.value,b.root.curve,b.value.saved.device),second=await c.sealMessage(two,a.value,b.root.curve,b.value.saved.device);assert.deepEqual(await c.openMessage(second,restored,a.root.curve,expected(two)),two);assert.deepEqual(await c.openMessage(first,restored,a.root.curve,expected(one)),one);await assert.rejects(c.openMessage(first,restored,a.root.curve,expected(one)));restored.engine.free();a.value.engine.free();b.value.engine.free();
});
test('tampered content, recipient, origin, device and burn policy are rejected',async()=>{
  const {a,b}=await pair(),v=envelope();const cipher=await c.sealMessage(v,a.value,b.root.curve,b.value.saved.device);const snapshot=b.value.engine.save();
  async function reject(packet,context){await assert.rejects(c.openMessage(packet,b.value,a.root.curve,context));b.value.engine.free();b.value.engine=m.Engine.restore(snapshot);}
  const damaged=JSON.parse(cipher);damaged.content=damaged.content.slice(0,-5)+'AAAA=';await reject(JSON.stringify(damaged),expected(v));
  for(const wrong of [{site:'https://attacker.example'},{recipient:3},{burn:true}])await reject(cipher,{...expected(v),...wrong});
  const device=JSON.parse(cipher);device.device=crypto.randomUUID();await reject(JSON.stringify(device),expected(v));assert.deepEqual(await c.openMessage(cipher,b.value,a.root.curve,expected(v)),v);a.value.engine.free();b.value.engine.free();
});
test('read-and-burn plaintext is never written to persistent cache; expired ordinary cache is purged',async()=>{
  const {a,b}=await pair(),v=envelope(undefined,true);const cipher=await c.sealMessage(v,a.value,b.root.curve,b.value.saved.device);await c.openMessage(cipher,b.value,a.root.curve,expected(v));b.value.saved.readPending=[v.id];b.value.saved.cache.old={value:{...v,burn:false,expires:1},created_at:1};await c.persist(b.value);const restored=await c.unlockState(b.backup,await c.loadLocal('keys','state:2'),2,b.backup.publicKey,phrase);assert.deepEqual(restored.saved.cache,{});assert.deepEqual(restored.saved.readPending,[v.id]);restored.engine.free();a.value.engine.free();b.value.engine.free();
});
test('malicious plaintext format is rejected rather than rendered as HTML or fetched remotely',()=>{
  const v=envelope();for(const change of [{text:'x'.repeat(4097)},{image:'https://tracker.example/pixel'},{image:'data:image/svg+xml;base64,PHN2Zz4='},{expires:Infinity},{sticker:'x'.repeat(33)}])assert.throws(()=>c.validEnvelope({...v,...change},expected(v)));assert.equal(c.validEnvelope({...v,text:'<script>not HTML</script>'},expected(v)).text,'<script>not HTML</script>');
});

const componentSource=await readFile(new URL('../src/Friends.tsx',import.meta.url),'utf8');
const componentFile=ts.createSourceFile('Friends.tsx',componentSource,ts.ScriptTarget.Latest,true,ts.ScriptKind.TSX);
function componentFunctions(names,globals){const functions=[];const visit=n=>{if(ts.isFunctionDeclaration(n)&&names.includes(n.name?.text))functions.push(n.getText(componentFile));ts.forEachChild(n,visit);};visit(componentFile);assert.equal(functions.length,names.length);const code=ts.transpileModule(functions.join('\n')+`;exports.functions={${names.join(',')}};`,{compilerOptions:{target:ts.ScriptTarget.ES2022}}).outputText;const context=vm.createContext({exports:{},...globals});vm.runInContext(code,context);return context.exports.functions;}
test('automatic device activation persists both leases and recovers a lost activation acknowledgement',async()=>{
  const backup=await c.createIdentity(13,phrase),state=await c.unlockState(backup,undefined,13,backup.publicKey,phrase);let serverLease=state.saved.lease,loseReply=true;const calls=[];
  const globals={crypto,persist:c.persist,privateApi:async(s,path,body)=>{assert.equal(path,'/device');const durable=await c.unlockState(backup,await c.loadLocal('keys','state:13'),13,backup.publicKey,phrase);assert.equal(durable.saved.lease,body.lease);assert.ok(durable.saved.previousLease);durable.engine.free();calls.push({header:s.saved.lease,next:body.lease});if(s.saved.lease!==serverLease)throw Object.assign(Error('old lease'),{status:409});serverLease=body.lease;if(loseReply){loseReply=false;throw Object.assign(Error('lost ACK'),{status:503});}return {};} };
  const {activateDevice}=componentFunctions(['activateDevice'],globals);await assert.rejects(activateDevice(state,''));const restored=await c.unlockState(backup,await c.loadLocal('keys','state:13'),13,backup.publicKey,phrase);const next=restored.saved.lease;await activateDevice(restored,'');assert.equal(calls.length,3);assert.equal(calls[0].next,next);assert.equal(calls[1].next,next);assert.equal(calls[2].header,next);assert.equal(restored.saved.previousLease,undefined);
  const durable=await c.unlockState(backup,await c.loadLocal('keys','state:13'),13,backup.publicKey,phrase);assert.equal(durable.saved.lease,serverLease);assert.equal(durable.saved.previousLease,undefined);durable.engine.free();restored.engine.free();state.engine.free();
});
test('forged server mode metadata alone cannot downgrade or decrypt an untrusted conversation',async()=>{
  const {a,b}=await pair();let status='',calls=0;const globals={me:{id:1,storage_enabled:true,retention_days:7},peer:{id:2,public_key:b.backup.publicKey,allow_server:true,peer_allow_server:true,mode_epoch:2},trusted:false,localServer:false,engine:{current:a.value},operation:{current:false},alive:{current:true},draft:'must remain private',image:'',burn:false,structuredClone,crypto,cryptoModule:c.cryptoModule,setBusy(){},setStatus(v){status=v;},finishOperation(){globals.operation.current=false;},privateApi:async()=>{calls++;},receive:async()=>{} };
  const functions=componentFunctions(['send','receive'],globals);await functions.send();await functions.receive();assert.match(status,/尚未明确同意/);assert.equal(calls,0);assert.equal(a.value.saved.outbox,undefined);a.value.engine.free();b.value.engine.free();
});
test('explicit local consent sends server-readable AES-256 content without an Olm prekey claim',async()=>{
  const {a,b}=await pair();const calls=[];const globals={me:{id:1,storage_enabled:true,retention_days:7},peer:{id:2,public_key:b.backup.publicKey,allow_server:true,peer_allow_server:true,mode_epoch:2},trusted:false,localServer:true,engine:{current:a.value},operation:{current:false},alive:{current:true},draft:'explicit consent fixture',image:'',burn:false,structuredClone,crypto,location:{origin:'https://fixture.example'},sealServerMessage:c.sealServerMessage,persist:c.persist,cryptoModule:c.cryptoModule,setBusy(){},setStatus(){},setDraft(){},setImage(){},finishOperation(){globals.operation.current=false;},receive:async()=>{},privateApi:async(_s,path,body)=>{assert.equal(path,'/messages');calls.push(body);return {};} };
  const functions=componentFunctions(['send','flushOutbox'],globals);await functions.send();assert.equal(calls.length,1);assert.equal(calls[0].mode,'server');assert.equal(calls[0].epoch,2);const plain=await c.openServerMessage(calls[0].ciphertext,2,{site:'https://fixture.example',id:calls[0].id,sender:1,recipient:2,burn:false});assert.equal(plain.text,globals.draft);a.value.engine.free();b.value.engine.free();
});
test('UI durable outbox retries identical ciphertext without rolling back or duplicating a draft',async()=>{
  const {a,b}=await pair();a.value.saved.peerDevices[2]=b.value.saved.device;const calls=[];let fail=true,draft='retry fixture',image='',status='';const globals={me:{id:1,storage_enabled:true,retention_days:7},peer:{id:2,public_key:b.backup.publicKey,device:b.value.saved.device},trusted:true,engine:{current:a.value},operation:{current:false},alive:{current:true},draft,image,burn:false,crypto,location:{origin:'https://fixture.example'},structuredClone,sealMessage:c.sealMessage,persist:c.persist,cryptoModule:c.cryptoModule,setBusy(){},setStatus(v){status=v;},setDraft(v){draft=v;globals.draft=v;},setImage(v){image=v;globals.image=v;},finishOperation(){globals.operation.current=false;},receive:async()=>{},privateApi:async(_s,path,body)=>{assert.equal(path,'/messages');calls.push(structuredClone(body));if(fail)throw Error('synthetic offline');return {};} };
  const functions=componentFunctions(['send','flushOutbox'],globals);await functions.send();assert.match(status,/待发箱/);assert.equal(draft,'');assert.equal(image,'');assert.ok(a.value.saved.outbox);const stored=await c.loadLocal('keys','state:1');const restored=await c.unlockState(a.backup,stored,1,a.backup.publicKey,phrase);assert.equal(restored.saved.outbox.ciphertext,calls[0].ciphertext);const engineBefore=a.value.engine.save();fail=false;await functions.flushOutbox(a.value);assert.equal(a.value.saved.outbox,undefined);assert.equal(a.value.engine.save(),engineBefore);assert.deepEqual(calls[0],calls[1]);const decoded=await c.openMessage(calls[1].ciphertext,b.value,a.root.curve,{site:'https://fixture.example',id:calls[1].id,sender:1,recipient:2,burn:false});assert.equal(decoded.text,'retry fixture');restored.engine.free();a.value.engine.free();b.value.engine.free();
});
test('UI persists incoming ratchet before failed burn acknowledgement and retries without decrypting twice',async()=>{
 const {a,b}=await pair();const first=envelope();await c.openMessage(await c.sealMessage(first,a.value,b.root.curve,b.value.saved.device),b.value,a.root.curve,expected(first));const v={...envelope(undefined,true),sender:2,recipient:1,text:'burn fixture'};const ciphertext=await c.sealMessage(v,b.value,a.root.curve,a.value.saved.device);const row={seq:1,id:v.id,sender:2,recipient:1,created_at:Date.now()/1000,expires:v.expires,burn:true,read_at:null};let failed=true,acked=false,rows=[];
 const globals={me:{id:1},peer:{id:2,public_key:b.backup.publicKey},trusted:true,engine:{current:a.value},operation:{current:false},alive:{current:true},selectedRef:{current:2},recordsRef:{current:rows},before:0,location:{origin:v.site},openMessage:c.openMessage,persist:c.persist,cryptoModule:c.cryptoModule,refresh:async()=>{},finishOperation(){globals.operation.current=false;},setStatus(){},setRecords(update){rows=update(rows);globals.recordsRef.current=rows;},privateApi:async(state,path)=>{if(path==='/2/messages')return {messages:acked?[]:[row]};if(path===`/messages/${v.id}`)return {ciphertext};assert.equal(path,`/messages/${v.id}/read`);const saved=await c.unlockState(a.backup,await c.loadLocal('keys','state:1'),1,a.backup.publicKey,phrase);assert.deepEqual(saved.saved.readPending,[v.id]);saved.engine.free();if(failed)throw Error('synthetic ACK timeout');acked=true;return {};} };
 const {receive}=componentFunctions(['receive'],globals);await receive();assert.equal(rows[0].value.text,'burn fixture');assert.deepEqual([...a.value.saved.readPending],[v.id]);assert.equal(a.value.saved.cache[v.id],undefined);failed=false;await receive();assert.equal(acked,true);assert.equal(a.value.saved.readPending.length,0);assert.equal(rows.length,1);assert.equal(rows[0].value.text,'burn fixture');await assert.rejects(c.openMessage(ciphertext,a.value,b.root.curve,expected(v)));a.value.engine.free();b.value.engine.free();
});
