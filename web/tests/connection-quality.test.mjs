import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/connection-quality.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {connectionQuality,QualityHistory}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
test('selected transport metrics use milliseconds and cumulative weighted receive counters',()=>{
  const report=new Map([['transport',{type:'transport',selectedCandidatePairId:'active'}],['old',{type:'candidate-pair',state:'succeeded',nominated:true,currentRoundTripTime:9}],['active',{type:'candidate-pair',currentRoundTripTime:.025,localCandidateId:'private-address'}],['audio',{type:'inbound-rtp',kind:'audio',packetsReceived:98,packetsLost:2,jitter:.003,jitterBufferDelay:4,jitterBufferEmittedCount:100}],['video',{type:'inbound-rtp',kind:'video',packetsReceived:1,packetsLost:900,jitter:10}]]);
  assert.deepEqual(connectionQuality(report),{rtt:25,jitter:3,loss:2,buffer:40,received:98});
});
test('missing, invalid and empty counters remain unreported; recovered loss never becomes negative',()=>{
  assert.deepEqual(connectionQuality(new Map()),{});
  assert.deepEqual(connectionQuality(new Map([['a',{type:'inbound-rtp',kind:'audio',packetsReceived:0,packetsLost:0,jitterBufferDelay:0,jitterBufferEmittedCount:0}]])),{received:0});
  assert.deepEqual(connectionQuality(new Map([['a',{type:'inbound-rtp',kind:'audio',packetsReceived:1,packetsLost:-1,jitter:NaN}]])),{loss:0,received:1});
});

test('relay detection and packet summaries never export candidate addresses or identifiers',()=>{
 const stats=new Map([['t',{type:'transport',selectedCandidatePairId:'p'}],['p',{type:'candidate-pair',localCandidateId:'l',remoteCandidateId:'r'}],['l',{type:'local-candidate',candidateType:'relay',address:'192.168.1.1',usernameFragment:'secret'}],['r',{type:'remote-candidate',candidateType:'host',address:'10.0.0.1'}],['a',{type:'outbound-rtp',kind:'audio',packetsSent:42}]]);
 assert.deepEqual(connectionQuality(stats),{route:'relay',sent:42});stats.get('l').candidateType='srflx';assert.equal(connectionQuality(stats).route,'direct');
});

const sample=(rx,lost,delay,emitted,extra={})=>new Map([['audio',{type:'inbound-rtp',kind:'audio',ssrc:7,packetsReceived:rx,packetsLost:lost,jitterBufferDelay:delay,jitterBufferEmittedCount:emitted,...extra}]]);
test('recent receive metrics discard old loss and use delta-weighted playback counters',()=>{
 const history=new QualityHistory();assert.equal(history.update(sample(900,100,10,100),0).recentLoss,undefined);
 let result=history.update(sample(950,100,11,150,{totalSamplesReceived:48000,concealedSamples:0,concealmentEvents:0}),2000);
 assert.equal(result.recentLoss,0);assert.equal(result.recentBuffer,20);assert.equal(result.windowSeconds,2);
 result=history.update(sample(998,102,13,200,{totalSamplesReceived:96000,concealedSamples:4800,concealmentEvents:2}),4000);
 assert.equal(result.recentLoss,2);assert.equal(result.recentBuffer,30);assert.equal(result.recentConcealed,10);assert.equal(result.recentConcealmentEvents,2);
 for(let i=3;i<=7;i++)result=history.update(sample(998+(i-2)*50,102,13+(i-2),200+(i-2)*50),i*2000);
 assert.equal(result.recentLoss,0);assert.equal(result.recentBuffer,20);assert.equal(result.windowSeconds,10);
 assert.ok(!JSON.stringify(result).includes('ssrc'));
});
test('counter resets, background gaps, missing streams and timestamps never fabricate recent quality',()=>{
 const history=new QualityHistory();history.update(sample(100,5,2,100),100);
 let result=history.update(sample(0,0,0,0),2100);assert.equal(result.recentLoss,undefined);assert.equal(result.recentBuffer,undefined);
 result=history.update(sample(50,-1,1,50),4100);assert.equal(result.recentLoss,0);
 result=history.update(sample(100,0,2,100),30000);assert.equal(result.recentLoss,undefined);assert.equal(result.windowSeconds,undefined);
 history.update(new Map(),32000);result=history.update(sample(200,0,4,200),34000);assert.equal(result.recentLoss,undefined);
 assert.equal(history.update(sample(500,0,10,500),NaN).recentLoss,undefined);
});
