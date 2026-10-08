// Runs inside the RNNoise worklet on 480-sample/10ms denoised frames.
// The probability comes from RNNoise, never from an amplitude threshold.
export function createVoiceGate(onchange) {
  let hold=0,gain=0,speaking=false;
  const previous=new Float32Array(480);
  return (frame,probability)=>{
    if(!Number.isFinite(probability)||probability<0||probability>1)throw new Error('Invalid speech probability');
    if(probability>=.6||(hold>0&&probability>=.35))hold=20;
    const open=hold>0,active=open||gain>0;
    if(active!==speaking){speaking=active;onchange?.(active);}
    hold=Math.max(0,hold-1);
    for(let i=0;i<frame.length;i++){
      gain=open?Math.min(1,gain+1/240):Math.max(0,gain-1/480);
      const sample=frame[i];
      frame[i]=previous[i]*gain;
      previous[i]=sample;
    }
  };
}

// Transmission detection is deliberately more permissive than voice-only filtering.
// Keep complete syllables/pauses intact; this detector never changes PCM samples.
export function createVoiceActivity(onchange) {
  let hold=0,attack=0,speaking=false;
  return probability=>{
    if(!Number.isFinite(probability)||probability<0||probability>1)throw new Error('Invalid speech probability');
    attack=probability>=.2?attack+1:0;
    if(probability>=.6||attack>=2||(speaking&&probability>=.1))hold=40;
    const active=hold>0;
    if(active!==speaking){speaking=active;onchange?.(active);}
    hold=Math.max(0,hold-1);
    return active;
  };
}
