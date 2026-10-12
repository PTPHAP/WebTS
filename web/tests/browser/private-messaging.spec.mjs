import {test,expect} from '@playwright/test';

const fixture='/tests/browser/private-messaging.html';
async function start(page){await page.goto(fixture);await page.waitForFunction(()=>!!window.cryptoClient);}

test('automatic keys, encrypted ratchet and outbox survive a real browser reload',async({page})=>{
  await start(page);
  const before=await page.evaluate(async()=>{
    const c=window.cryptoClient,identity=await c.createAutomaticIdentity(101);
    await c.saveLocal('keys','identity:101',identity);
    const state=await c.unlockState(identity,undefined,101,identity.publicKey,'');
    state.engine.maintenance(Date.now()/1000,JSON.stringify(Array.from({length:132},(_,i)=>`synthetic-peer-${i}`)));
    state.saved.outbox={peer:102,id:'synthetic pending',ciphertext:'already encrypted fixture',burn:false};
    await c.persist(state);const device=await c.loadLocal('keys','device:101');
    state.engine.free();return {key:identity.publicKey,deviceExtractable:device.extractable};
  });
  expect(before.deviceExtractable).toBe(false);await page.reload();await page.waitForFunction(()=>!!window.cryptoClient);
  const after=await page.evaluate(async()=>{
    const c=window.cryptoClient,identity=await c.loadLocal('keys','identity:101'),record=await c.loadLocal('keys','state:101');
    const state=await c.unlockState(identity,record,101,identity.publicKey,'');
    const result={key:state.engine.identity(),pending:state.saved.outbox.ciphertext,encrypted:!JSON.stringify(record).includes('already encrypted fixture')};
    state.engine.free();return result;
  });
  expect(after).toEqual({key:before.key,pending:'already encrypted fixture',encrypted:true});
});

test('two isolated browser devices exchange text, images and stickers with replay rejection',async({browser})=>{
  const contexts=await Promise.all([browser.newContext(),browser.newContext()]);
  try{
    const pages=await Promise.all(contexts.map(c=>c.newPage()));await Promise.all(pages.map(start));
    async function actor(page,id){return page.evaluate(async id=>{
      const c=window.cryptoClient,identity=await c.createAutomaticIdentity(id),state=await c.unlockState(identity,undefined,id,identity.publicKey,'');
      window.actor=state;const bundle=JSON.parse(state.engine.prekeys());await c.persist(state);
      return {identity:identity.publicKey,device:state.saved.device,key:JSON.parse(bundle.payload).keys[0],bundle};
    },id);}
    const [a,b]=await Promise.all([actor(pages[0],101),actor(pages[1],102)]);
    await pages[0].evaluate(async b=>{const c=window.cryptoClient,target=await c.checkPrekey(b.identity,{key:b.key,bundle:{...b.bundle,device:b.device}});window.actor.engine.outbound(target.curve,b.key);},b);
    async function seal(page,peer,sender,recipient,text){return page.evaluate(async({peer,sender,recipient,text})=>{
      const c=window.cryptoClient,value={version:1,site:location.origin,id:crypto.randomUUID(),sender,recipient,expires:Math.floor(Date.now()/1000)+3600,text,image:'data:image/jpeg;base64,c2FmZQ==',sticker:'hello',burn:true,burn_seconds:600};
      const ciphertext=await c.sealMessage(value,window.actor,JSON.parse(peer.identity).curve,peer.device);await c.persist(window.actor);return {value,ciphertext};
    },{peer,sender,recipient,text});}
    async function open(page,peer,packet){return page.evaluate(async({peer,packet})=>{
      const c=window.cryptoClient,v=packet.value,expected={site:location.origin,id:v.id,sender:v.sender,recipient:v.recipient,burn:true,burn_seconds:600};
      const decoded=await c.openMessage(packet.ciphertext,window.actor,JSON.parse(peer.identity).curve,expected);await c.persist(window.actor);return decoded;
    },{peer,packet});}
    const first=await seal(pages[0],b,101,102,'first private synthetic message');
    const second=await seal(pages[0],b,101,102,'second private synthetic message');
    expect(first.ciphertext).not.toContain(first.value.text);
    expect(await open(pages[1],a,second)).toEqual(second.value);
    expect(await open(pages[1],a,first)).toEqual(first.value);
    await expect(open(pages[1],a,first)).rejects.toThrow();
    const reply=await seal(pages[1],a,102,101,'reply synthetic');expect(await open(pages[0],b,reply)).toEqual(reply.value);
  }finally{await Promise.all(contexts.map(c=>c.close()));}
});

test('an aborted real IndexedDB transaction retains old identity and staged reset for retry',async({page})=>{
  await start(page);
  expect(await page.evaluate(async()=>{
    const c=window.cryptoClient,old=await c.createAutomaticIdentity(101),state=await c.unlockState(old,undefined,101,old.publicKey,'');
    await c.saveLocal('keys','identity:101',old);await c.persist(state);
    const record=await c.loadLocal('keys','state:101'),next=await c.createAutomaticIdentity(101);
    await c.saveIdentityReset({previousPublicKey:old.publicKey,identity:next});
    const put=IDBObjectStore.prototype.put;let aborted=false;
    IDBObjectStore.prototype.put=function(...args){const r=put.apply(this,args);if(!aborted){aborted=true;this.transaction.abort();}return r;};
    let failed=false;try{await c.completeIdentityReset(next);}catch{failed=true;}finally{IDBObjectStore.prototype.put=put;}
    const unchanged=JSON.stringify(await c.loadLocal('keys','identity:101'))===JSON.stringify(old)&&JSON.stringify(await c.loadLocal('keys','state:101'))===JSON.stringify(record);
    const staged=(await c.loadLocal('keys','reset:101')).identity.publicKey===next.publicKey;
    await c.completeIdentityReset(next);
    const committed=(await c.loadLocal('keys','identity:101')).publicKey===next.publicKey&&await c.loadLocal('keys','state:101')===undefined&&await c.loadLocal('keys','reset:101')===undefined;
    state.engine.free();return {failed,unchanged,staged,committed};
  })).toEqual({failed:true,unchanged:true,staged:true,committed:true});
});

test('loss of the automatic key can be recovered only with the portable backup',async({page})=>{
  await start(page);
  expect(await page.evaluate(async()=>{
    const c=window.cryptoClient,phrase='a long synthetic portable recovery phrase',identity=await c.createAutomaticIdentity(101);
    await c.saveLocal('keys','identity:101',identity);const portable=await c.exportIdentity(identity,phrase);
    await c.saveLocal('keys','device:101',undefined);
    let refused=false;try{const bad=await c.unlockState(identity,undefined,101,identity.publicKey,'');bad.engine.free();}catch{refused=true;}
    await c.saveImportedIdentity(portable);const restored=await c.unlockState(portable,undefined,101,identity.publicKey,phrase);
    const same=restored.engine.identity()===identity.publicKey;restored.engine.free();return {refused,same};
  })).toEqual({refused:true,same:true});
});

test('another tab cannot acquire the active private device lock until it is released',async({page,context})=>{
  await start(page);const other=await context.newPage();await start(other);
  await page.evaluate(()=>new Promise(resolve=>{navigator.locks.request('webts-olm-101',async()=>{resolve();await new Promise(done=>{window.releaseFixture=done;});});}));
  const available=()=>other.evaluate(()=>navigator.locks.request('webts-olm-101',{ifAvailable:true},lock=>!!lock));
  expect(await available()).toBe(false);await page.evaluate(()=>window.releaseFixture());
  await expect.poll(available).toBe(true);await other.close();
});
