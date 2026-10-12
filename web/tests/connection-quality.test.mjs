import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/connection-quality.ts',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022}}).outputText;
const {connectionQuality}=await import(`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`);
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
