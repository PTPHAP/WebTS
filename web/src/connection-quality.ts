export type ConnectionQuality={rtt?:number;jitter?:number;buffer?:number;loss?:number};
// Only numeric summaries reach the UI. Candidate addresses and raw reports stay local.
export function connectionQuality(report:RTCStatsReport):ConnectionQuality {
  const rows=Array.from(report.values());
  const selected=rows.find(r=>r.type==='transport'&&r.selectedCandidatePairId)?.selectedCandidatePairId;
  const pair=selected?report.get(selected):rows.find(r=>r.type==='candidate-pair'&&r.state==='succeeded'&&(r.selected||r.nominated));
  const finite=(n:unknown):n is number=>typeof n==='number'&&Number.isFinite(n)&&n>=0;
  const result:ConnectionQuality={};
  if(finite(pair?.currentRoundTripTime))result.rtt=pair.currentRoundTripTime*1000;
  let received=0,lost=0,delay=0,emitted=0;
  for(const row of rows){
    if(row.type!=='inbound-rtp'||(row.kind??row.mediaType)!=='audio')continue;
    if(finite(row.jitter))result.jitter=Math.max(result.jitter??0,row.jitter*1000);
    if(finite(row.packetsReceived)&&typeof row.packetsLost==='number'&&Number.isFinite(row.packetsLost)){received+=row.packetsReceived;lost+=Math.max(0,row.packetsLost);}
    if(finite(row.jitterBufferDelay)&&finite(row.jitterBufferEmittedCount)){delay+=row.jitterBufferDelay;emitted+=row.jitterBufferEmittedCount;}
  }
  if(received+lost>0)result.loss=lost/(received+lost)*100;
  if(emitted>0)result.buffer=delay/emitted*1000;
  return result;
}
