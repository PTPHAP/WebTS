// Runs inside the RNNoise worklet on 480-sample/10ms denoised frames.
// The probability comes from RNNoise, never from an amplitude threshold.
export function createVoiceGate() {
  let hold=0,gain=0;
  const previous=new Float32Array(480);
  return (frame,probability)=>{
    if(!Number.isFinite(probability)||probability<0||probability>1)throw new Error('Invalid speech probability');
    if(probability>=.6||(hold>0&&probability>=.35))hold=20;
    const open=hold>0;
    hold=Math.max(0,hold-1);
    for(let i=0;i<frame.length;i++){
      gain=open?Math.min(1,gain+1/240):Math.max(0,gain-1/480);
      const sample=frame[i];
      frame[i]=previous[i]*gain;
      previous[i]=sample;
    }
  };
}
