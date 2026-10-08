type Event = Record<string, unknown>;
import type {AudioSettings} from './audio-settings';
import type {NoiseProcessor} from './noise';
import {connectionQuality} from './connection-quality';
export class Voice {
  private socket?: WebSocket;
  private peer?: RTCPeerConnection;
  private stream?: MediaStream;
  private sendTrack?: MediaStreamTrack;
  private processed?: MediaStream;
  private context?: AudioContext;
  private timer?: number;
  private qualityTimer?: number;
  private noiseNode?: NoiseProcessor;
  private settings:AudioSettings={noise:'rnnoise',keyboard:true,voiceOnly:true,echo:true,autoGain:true,gain:1,volume:1};
  private elements = new Map<string,HTMLAudioElement>();
  private tracks = new Map<string,number>();
  private volumes = new Map<number,number>();
  private candidates: RTCIceCandidateInit[] = [];
  private active = false;
  private processingBlocked = false;
  private muted = false;
  private deafened = false;
  private pressed = false;
  private mode: 'ptt'|'open' = 'open';
  private sink = '';
  private generation = 0;
  private handling = Promise.resolve();
  constructor(private event: (event: Event) => void) {}
  send(event: Event) { if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify(event)); }
  async connect(request: Event, device: string, listenOnly: boolean) {
    this.close(); const generation = this.generation;
    try {
    const response = await fetch('/api/rtc'); if (!response.ok) throw new Error('登录已过期，请重新登录');
    const config = await response.json();if(generation!==this.generation)return;
    let stream: MediaStream | undefined;
    if (!listenOnly) {
      if (!navigator.mediaDevices?.getUserMedia) throw new Error('当前环境不能打开麦克风，请使用 HTTPS 和支持语音的浏览器，或勾选“仅收听”。');
      try { stream = await navigator.mediaDevices.getUserMedia({audio:{deviceId:device?{exact:device}:undefined,echoCancellation:this.settings.echo,noiseSuppression:false,autoGainControl:this.settings.autoGain,channelCount:1}}); }
      catch (error) {
        const name = error instanceof DOMException ? error.name : '';
        if (name === 'NotAllowedError' || name === 'SecurityError') throw new Error('麦克风权限被拒绝：请在浏览器地址栏的网站权限及系统设置中允许麦克风，再重新连接；也可勾选“仅收听”。');
        if (name === 'NotFoundError' || name === 'OverconstrainedError') throw new Error('未找到所选麦克风，请在语音设置中选择系统默认设备，或勾选“仅收听”。');
        if (name === 'NotReadableError') throw new Error('无法读取麦克风，请检查设备连接、系统权限及其他应用的占用，或勾选“仅收听”。');
        throw new Error('无法打开麦克风，请检查浏览器和系统的麦克风设置，或勾选“仅收听”。');
      }
    }
    if (generation !== this.generation) { stream?.getTracks().forEach(t=>t.stop()); return; }
    this.stream=stream;
    let sendStream=stream;
    let processing:string=this.settings.noise;
    if(stream&&(this.settings.noise==='rnnoise'||this.settings.gain!==1)) {
      const context=new AudioContext({latencyHint:'interactive',sampleRate:48000});this.context=context;
      const source=context.createMediaStreamSource(stream),gain=context.createGain(),destination=context.createMediaStreamDestination();
      this.processed=destination.stream;gain.gain.value=this.settings.gain;gain.connect(destination);
      const voiceOnly=this.settings.voiceOnly===true;
      const noiseFailed=()=>{source.disconnect();this.noiseNode?.destroy();this.noiseNode=undefined;this.processingBlocked=voiceOnly;if(voiceOnly){gain.gain.value=0;this.update();}else source.connect(gain);const mode=voiceOnly?'blocked':'off';this.event({type:'audio_processing',noise:mode,actual:stream?.getAudioTracks()[0].getSettings?.()});this.event({type:'notice',message:voiceOnly?'本地人声识别不可用，已暂停麦克风发送以免传出原始杂音；仍可收听。请重新连接重试，或断开后关闭“仅保留人声”。':'本地降噪暂不可用，已关闭降噪并恢复直接音频传输。请重新连接以重试。'});return mode;};
      if(this.settings.noise==='rnnoise') {
        try {
          const {createNoise}=await import('./noise');const node=await createNoise(context,this.settings.keyboard,voiceOnly);
          if(generation!==this.generation){node.destroy();return;}
          this.noiseNode=node;source.connect(node.input);node.output.connect(gain);processing=node.mode;
          if(node.warning)this.event({type:'notice',message:node.warning});
          node.onchange=message=>{if(generation!==this.generation)return;this.event({type:'audio_processing',noise:node.mode,voice_only:voiceOnly,actual:stream?.getAudioTracks()[0].getSettings?.()});this.event({type:'notice',message});};
          node.onerror=()=>{if(generation!==this.generation)return;noiseFailed();};
        } catch {
          if(generation!==this.generation)return;
          processing=noiseFailed();
        }
      } else source.connect(gain);
      await context.resume();sendStream=destination.stream;
    }
    if(generation!==this.generation){sendStream?.getTracks().forEach(t=>t.stop());return;}
    this.event({type:'audio_processing',noise:listenOnly?'listen':processing,voice_only:this.settings.noise==='rnnoise'&&this.settings.voiceOnly&&!this.processingBlocked,actual:stream?.getAudioTracks()[0].getSettings?.()});
    this.peer = new RTCPeerConnection({...config,bundlePolicy:'max-bundle'});
    const qualityPeer=this.peer;let reading=false;
    this.qualityTimer=window.setInterval(async()=>{if(reading||!qualityPeer.getStats)return;reading=true;try{const report=await qualityPeer.getStats();if(generation===this.generation)this.event({type:'quality',...connectionQuality(report)});}catch{if(generation===this.generation)this.event({type:'quality'});}finally{reading=false;}},2000);
    this.peer.onicecandidate=e=>{if(generation===this.generation&&e.candidate)this.send({type:'ice',candidate:e.candidate.toJSON()});};
    this.peer.ontrack=e=>{if(generation!==this.generation)return; const audio = new Audio(); audio.autoplay=true; audio.srcObject=new MediaStream([e.track]); this.elements.set(e.track.id,audio); this.applyAudio(); audio.play().catch(()=>this.event({type:'notice',message:'浏览器暂停了音频，请点击“启用收听”。'})); e.track.onended=()=>{audio.pause();audio.srcObject=null;this.elements.delete(e.track.id);}; };
    if(sendStream){this.sendTrack=sendStream.getAudioTracks()[0].clone();this.sendTrack.enabled=false;this.peer.addTrack(this.sendTrack,new MediaStream([this.sendTrack]));this.update();}
    if (sendStream) this.detect(sendStream);
    const socket = new WebSocket(`${location.protocol==='https:'?'wss':'ws'}://${location.host}/api/connect`); this.socket=socket;
    socket.onopen=()=>{if(generation===this.generation){socket.send(JSON.stringify(request));this.send({type:'mute',muted:this.muted,deafened:this.deafened});this.send({type:'transmit',enabled:this.active});}};
    socket.onmessage=e=>{this.handling=this.handling.then(async()=>{
      if(generation!==this.generation)return;
      const message=JSON.parse(e.data); const peer=this.peer; if(!peer)return;
      if(message.type==='offer'){await peer.setRemoteDescription(message.description);if(generation!==this.generation)return;for(const c of this.candidates)await peer.addIceCandidate(c);if(generation!==this.generation)return;this.candidates=[];const answer=await peer.createAnswer();if(generation!==this.generation)return;await peer.setLocalDescription(answer);if(generation!==this.generation)return;this.send({type:'answer',description:answer});}
      else if(message.type==='ice'){if(peer.remoteDescription)await peer.addIceCandidate(message.candidate);else this.candidates.push(message.candidate);}
      else if(message.type==='track'){this.tracks.set(message.track,message.client);this.applyAudio();}
      if(generation===this.generation)this.event(message);
    }).catch(()=>{if(generation!==this.generation)return;this.event({type:'error',message:'语音协商失败，请检查浏览器及网络设置。'});this.close();});};
    socket.onclose=()=>{if(generation===this.generation){this.close();this.event({type:'disconnected'});}};
    socket.onerror=()=>{if(generation===this.generation)this.event({type:'error',message:'无法连接网关，请检查网络。'});};
    } catch(error){if(generation!==this.generation)return;this.close();throw error;}
  }
  private detect(stream: MediaStream) {
    this.context??=new AudioContext({latencyHint:'interactive'});const source=this.context.createMediaStreamSource(stream);const analyser=this.context.createAnalyser();analyser.fftSize=512;source.connect(analyser);const data=new Float32Array(analyser.fftSize);
    this.timer=window.setInterval(()=>{analyser.getFloatTimeDomainData(data); const rms=Math.sqrt(data.reduce((sum,n)=>sum+n*n,0)/data.length);this.event({type:'level',level:Math.min(1,rms*8)});},30);
  }
  private update() {const active=!!this.sendTrack&&!this.processingBlocked&&!this.muted&&(this.mode==='open'||(this.pressed&&document.hasFocus()));if(active!==this.active){this.active=active;if(this.sendTrack)this.sendTrack.enabled=active;this.send({type:'transmit',enabled:active});this.event({type:'transmit',enabled:active});}}
  press(value:boolean){this.pressed=value;this.update();}
  setMode(mode:'ptt'|'open'){this.mode=mode;this.pressed=false;this.update();}
  configure(settings:AudioSettings){this.settings={...settings};this.applyAudio();}
  outputVolume(value:number){this.settings.volume=Math.max(0,Math.min(1,value));this.applyAudio();}
  setMute(muted:boolean,deafened:boolean){this.muted=muted;this.deafened=deafened;this.update();this.applyAudio();this.send({type:'mute',muted,deafened});}
  volume(client:number,value:number){this.volumes.set(client,value);this.applyAudio();}
  async output(id:string){this.sink=id;for(const element of this.elements.values()){const audio=element as HTMLAudioElement & {setSinkId?:(id:string)=>Promise<void>};if(audio.setSinkId)await audio.setSinkId(id);}}
  private applyAudio(){for(const[id,audio]of this.elements){audio.muted=this.deafened;audio.volume=(this.volumes.get(this.tracks.get(id)??0)??1)*this.settings.volume;const element=audio as HTMLAudioElement & {setSinkId?:(id:string)=>Promise<void>};if(this.sink&&element.setSinkId)element.setSinkId(this.sink).catch(()=>{});}}
  resume(){this.context?.resume();for(const audio of this.elements.values())audio.play().catch(()=>{});}
  close(){this.generation++;if(this.qualityTimer)clearInterval(this.qualityTimer);this.qualityTimer=undefined;this.handling=Promise.resolve();this.pressed=false;this.active=false;this.processingBlocked=false;this.send({type:'disconnect'});const socket=this.socket;this.socket=undefined;if(socket){socket.onclose=null;socket.close();}this.peer?.close();this.peer=undefined;this.sendTrack?.stop();this.sendTrack=undefined;this.processed?.getTracks().forEach(t=>t.stop());this.processed=undefined;this.stream?.getTracks().forEach(t=>t.stop());this.stream=undefined;if(this.timer)clearInterval(this.timer);this.timer=undefined;this.noiseNode?.destroy();this.noiseNode=undefined;this.context?.close();this.context=undefined;for(const audio of this.elements.values()){audio.pause();audio.srcObject=null;}this.elements.clear();this.tracks.clear();this.volumes.clear();this.candidates=[];}
}
