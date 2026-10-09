export type SoundSettings={enabled:boolean;volume:number};
export type Sound='connect'|'disconnect'|'join'|'leave'|'message'|'poke'|'mail'|'on'|'off'|'error'|'away'|'back';
const tones:Record<Sound,number[]>={away:[660,520,390],back:[390,520,780],connect:[440,660],disconnect:[440,300],join:[620,780],leave:[520,390],message:[880,1175],poke:[660,990,1320,990],mail:[784,988,1175],on:[550,700],off:[550,400],error:[240,190]};
export function readSoundSettings():SoundSettings {
  try{const v=JSON.parse(localStorage.getItem('webts-sounds')??'{}');return {enabled:v.enabled!==false,volume:Number.isFinite(v.volume)?Math.max(0,Math.min(1,v.volume)):.5};}catch{return {enabled:true,volume:.5};}
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
    const prominent=kind==='poke'||kind==='mail'||kind==='message',duration=prominent ? .2 : .08,gap=prominent ? .22 : .09;
    tones[kind].forEach((frequency,i)=>{const start=context.currentTime+i*gap,osc=context.createOscillator(),gain=context.createGain();osc.frequency.value=frequency;gain.gain.setValueAtTime(0,start);gain.gain.linearRampToValueAtTime(this.settings.volume*(kind==='poke' ? .55 : prominent ? .4 : .15),start+.012);gain.gain.exponentialRampToValueAtTime(.0001,start+duration);osc.connect(gain);gain.connect(context.destination);osc.start(start);osc.stop(start+duration+.005);osc.onended=()=>{osc.disconnect();gain.disconnect();};});
  }
  close(){void this.context?.close().catch(()=>{});this.context=undefined;this.last.clear();}
}
