export type SoundSettings={enabled:boolean;volume:number};
export type Sound='connect'|'disconnect'|'join'|'leave'|'message'|'poke'|'on'|'off'|'error';
const tones:Record<Sound,number[]>={connect:[440,660],disconnect:[440,300],join:[620,780],leave:[520,390],message:[740],poke:[660,880,660],on:[550,700],off:[550,400],error:[240,190]};
export function readSoundSettings():SoundSettings {
  try{const v=JSON.parse(localStorage.getItem('webts-sounds')??'{}');return {enabled:v.enabled!==false,volume:Number.isFinite(v.volume)?Math.max(0,Math.min(1,v.volume)):.15};}catch{return {enabled:true,volume:.15};}
}
export class NotificationSounds {
  private context?:AudioContext;
  private last=new Map<Sound,number>();
  constructor(private settings:SoundSettings){}
  configure(settings:SoundSettings){this.settings={...settings};}
  // Called only from a real user gesture; never request microphone permission.
  arm(){if(!this.settings.enabled)return;try{this.context??=new AudioContext({latencyHint:'interactive'});void this.context.resume().catch(()=>{});}catch{/* Sound support must not block connection. */}}
  play(kind:Sound){
    const context=this.context,now=Date.now();
    if(!this.settings.enabled||this.settings.volume<=0||context?.state!=='running'||now-(this.last.get(kind)??0)<600)return;
    this.last.set(kind,now);
    tones[kind].forEach((frequency,i)=>{const start=context.currentTime+i*.09,osc=context.createOscillator(),gain=context.createGain();osc.frequency.value=frequency;gain.gain.setValueAtTime(0,start);gain.gain.linearRampToValueAtTime(this.settings.volume*.15,start+.008);gain.gain.exponentialRampToValueAtTime(.0001,start+.08);osc.connect(gain);gain.connect(context.destination);osc.start(start);osc.stop(start+.085);osc.onended=()=>{osc.disconnect();gain.disconnect();};});
  }
  close(){void this.context?.close().catch(()=>{});this.context=undefined;this.last.clear();}
}
