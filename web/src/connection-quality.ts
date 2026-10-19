export type ConnectionQuality={rtt?:number;jitter?:number;buffer?:number;loss?:number;received?:number;sent?:number;route?:'direct'|'relay';statsAvailable?:boolean;inboundStreams?:number;audioInboundStreams?:number;windowSeconds?:number;recentLoss?:number;recentBuffer?:number;recentConcealed?:number;recentConcealmentEvents?:number};
function audioStats(row:RTCStats&{kind?:string;mediaType?:string;codecId?:string},report:RTCStatsReport):boolean {
  const kind=row.kind||row.mediaType;
  if(kind)return kind==='audio';
  const codec=row.codecId?report.get(row.codecId):undefined;
  return typeof codec?.mimeType==='string'&&codec.mimeType.toLowerCase().startsWith('audio/');
}
export function receiveStatsCoverage(report:RTCStatsReport){
  let inboundStreams=0,audioInboundStreams=0;
  for(const row of report.values())if(row.type==='inbound-rtp'){inboundStreams++;if(audioStats(row,report))audioInboundStreams++;}
  return {inboundStreams,audioInboundStreams};
}
// Only numeric summaries reach the UI. Candidate addresses and raw reports stay local.
export function connectionQuality(report:RTCStatsReport):ConnectionQuality {
  const rows=Array.from(report.values());
  const selected=rows.find(r=>r.type==='transport'&&r.selectedCandidatePairId)?.selectedCandidatePairId;
  const pair=selected?report.get(selected):rows.find(r=>r.type==='candidate-pair'&&r.state==='succeeded'&&(r.selected||r.nominated));
  const finite=(n:unknown):n is number=>typeof n==='number'&&Number.isFinite(n)&&n>=0;
  const result:ConnectionQuality={};
  const local=pair?.localCandidateId?report.get(pair.localCandidateId):undefined,remote=pair?.remoteCandidateId?report.get(pair.remoteCandidateId):undefined;
  if(local?.candidateType==='relay'||remote?.candidateType==='relay')result.route='relay';
  else if(local&&['host','srflx','prflx'].includes(local.candidateType)&&remote&&['host','srflx','prflx'].includes(remote.candidateType))result.route='direct';
  if(finite(pair?.currentRoundTripTime))result.rtt=pair.currentRoundTripTime*1000;
  let received=0,lost=0,delay=0,emitted=0;
  for(const row of rows){
    if(row.type==='outbound-rtp'&&audioStats(row,report)&&finite(row.packetsSent))result.sent=(result.sent??0)+row.packetsSent;
    if(row.type!=='inbound-rtp'||!audioStats(row,report))continue;
    if(finite(row.jitter))result.jitter=Math.max(result.jitter??0,row.jitter*1000);
    if(finite(row.packetsReceived))result.received=(result.received??0)+row.packetsReceived;
    if(finite(row.packetsReceived)&&typeof row.packetsLost==='number'&&Number.isFinite(row.packetsLost)){received+=row.packetsReceived;lost+=Math.max(0,row.packetsLost);}
    if(finite(row.jitterBufferDelay)&&finite(row.jitterBufferEmittedCount)){delay+=row.jitterBufferDelay;emitted+=row.jitterBufferEmittedCount;}
  }
  if(received+lost>0)result.loss=lost/(received+lost)*100;
  if(emitted>0)result.buffer=delay/emitted*1000;
  return result;
}

type Counter='packetsReceived'|'packetsLost'|'jitterBufferDelay'|'jitterBufferEmittedCount'|'totalSamplesReceived'|'concealedSamples'|'concealmentEvents';
type Counters=Partial<Record<Counter,number>>;
type Period={from:number;to:number;packets:number;lost:number;delay:number;emitted:number;samples:number;concealed:number;events:number;hasEvents:boolean};
const counters:Counter[]=['packetsReceived','packetsLost','jitterBufferDelay','jitterBufferEmittedCount','totalSamplesReceived','concealedSamples','concealmentEvents'];
// Retain only numeric counters, never raw stats, candidates, addresses or device identifiers.
export class QualityHistory {
  private previous?:{at:number;streams:Map<string,Counters>};
  private periods:Period[]=[];
  reset(){this.previous=undefined;this.periods=[];}
  update(report:RTCStatsReport,now:number):ConnectionQuality {
    const result=connectionQuality(report),streams=new Map<string,Counters>();
    if(!Number.isFinite(now)||now<0){this.reset();return result;}
    report.forEach((row,id)=>{
      if(streams.size>=128||row.type!=='inbound-rtp'||!audioStats(row,report)||typeof id!=='string'||id.length>256)return;
      const value:Counters={};for(const key of counters){const n=row[key];if(typeof n==='number'&&Number.isFinite(n)&&Math.abs(n)<=Number.MAX_SAFE_INTEGER&&(n>=0||key==='packetsLost'))value[key]=n;}
      streams.set(`${id}:${Number.isSafeInteger(row.ssrc)?row.ssrc:''}`,value);
    });
    const old=this.previous;this.previous={at:now,streams};
    if(!old||now<=old.at||now-old.at>10000){this.periods=[];return result;}
    const period:Period={from:old.at,to:now,packets:0,lost:0,delay:0,emitted:0,samples:0,concealed:0,events:0,hasEvents:false};
    for(const [key,current] of streams){
      const before=old.streams.get(key);if(!before)continue;
      const delta=(name:Counter,signed=false)=>current[name]!==undefined&&before[name]!==undefined&&(signed||current[name]!>=before[name]!)?Math.max(0,current[name]!-before[name]!):undefined;
      const rx=delta('packetsReceived');if(rx===undefined)continue;
      const loss=delta('packetsLost',true);if(loss!==undefined){period.packets+=rx;period.lost+=loss;}
      const emitted=delta('jitterBufferEmittedCount'),delay=delta('jitterBufferDelay');if(emitted!==undefined&&delay!==undefined){period.emitted+=emitted;period.delay+=delay;}
      const total=delta('totalSamplesReceived'),concealed=delta('concealedSamples');if(total!==undefined&&concealed!==undefined){period.samples+=total;period.concealed+=Math.min(total,concealed);}
      const events=delta('concealmentEvents');if(events!==undefined){period.events+=events;period.hasEvents=true;}
    }
    this.periods=[...this.periods,period].filter(p=>p.from>=now-10000).slice(-16);
    result.windowSeconds=(now-this.periods[0].from)/1000;
    const sum=(field:'packets'|'lost'|'delay'|'emitted'|'samples'|'concealed'|'events')=>this.periods.reduce((total,p)=>total+p[field],0);
    const received=sum('packets'),lost=sum('lost'),emitted=sum('emitted'),samples=sum('samples');
    if(received+lost>0)result.recentLoss=100*lost/(received+lost);
    if(emitted>0)result.recentBuffer=1000*sum('delay')/emitted;
    if(samples>0)result.recentConcealed=100*sum('concealed')/samples;
    if(this.periods.some(p=>p.hasEvents))result.recentConcealmentEvents=sum('events');
    return result;
  }
}
