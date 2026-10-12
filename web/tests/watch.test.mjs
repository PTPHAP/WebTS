import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/watch.ts',import.meta.url),'utf8'),{compilerOptions:{target:ts.ScriptTarget.ES2022,module:ts.ModuleKind.ESNext}}).outputText;
const {playerUrl,officialLink,emptyWatch,scoreFreshness,watchStages,stageConfig}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('only recognized official embeds are allowed and require explicit enablement',()=>{
 const c={...emptyWatch,enabled:true,embed_enabled:true,url:'https://live.bilibili.com/123'};
 assert.equal(new URL(playerUrl(c,'example.com')).hostname,'www.bilibili.com');
 assert.equal(new URL(playerUrl({...c,url:'https://www.youtube.com/watch?v=abcdefghijk'},'example.com')).hostname,'www.youtube-nocookie.com');
 const twitch=new URL(playerUrl({...c,url:'https://www.twitch.tv/example'},'web.example.com'));assert.equal(twitch.searchParams.get('parent'),'web.example.com');assert.equal(twitch.searchParams.get('autoplay'),'false');
 for(const url of ['https://live.bilibili.com.evil.invalid/123','https://www.huya.com/123','javascript:alert(1)','https://u:p@live.bilibili.com/123','https://www.youtube.com:8443/watch?v=abcdefghijk','https://live.bilibili.com/123/other'])assert.equal(playerUrl({...c,url},'example.com'),null);
 assert.equal(playerUrl({...c,embed_enabled:false},'example.com'),null);assert.equal(playerUrl({...c,enabled:false},'example.com'),null);
});
test('official links cannot inject scripts or credentials and stale scores remain marked stale',()=>{
 for(const url of ['javascript:alert(1)','data:text/html,test','http://example.com','https://u:p@example.com'])assert.equal(officialLink(url),null);
 assert.equal(officialLink('https://example.com/room'),'https://example.com/room');
 assert.equal(scoreFreshness({updated_at:100},120,9),true);assert.equal(scoreFreshness({updated_at:100},120,11),false);assert.equal(scoreFreshness(null,120,0),false);
});

test('Bilibili canonical room links use the resolved real room ID and documented controls',()=>{
 const c={...emptyWatch,enabled:true,embed_enabled:true,url:'https://live.bilibili.com/21686237'};
 const u=new URL(playerUrl(c,'example.com'));
 assert.equal(u.searchParams.get('cid'),'21686237');
 assert.equal(u.searchParams.get('quality'),'1');
 assert.equal(playerUrl({...c,url:'https://live.bilibili.com/0'},'example.com'),null);
 assert.equal(playerUrl({...c,url:'https://live.bilibili.com/999999999999999999'},'example.com'),null);
});

test('legacy primary and additional stages stay separate and a removed stage falls back to main',()=>{
 const c={...emptyWatch,enabled:true,url:'https://live.bilibili.com/123',embed_enabled:true,stages:[{id:'secondary',name:'副舞台',url:'https://www.youtube.com/watch?v=abcdefghijk',cover:'',embed_enabled:false}]};
 assert.deepEqual(watchStages(c).map(s=>s.id),['main','secondary']);
 assert.equal(stageConfig(c,'secondary').url,c.stages[0].url);
 assert.equal(playerUrl(stageConfig(c,'secondary'),'example.com'),null);
 assert.equal(stageConfig(c,'deleted').url,c.url);
 assert.equal(watchStages(emptyWatch).length,1);
});
