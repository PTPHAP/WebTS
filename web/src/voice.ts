type Event = Record<string, unknown>;
import {defaultAudioSettings,type AudioSettings} from './audio-settings';
import {createReceiver} from './receive-audio';
import type {NoiseProcessor} from './noise';
import {connectionQuality} from './connection-quality';
class Retryable extends Error {}
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
  private settings:AudioSettings={...defaultAudioSettings};
  private receivers=new Map<string,ReturnType<typeof createReceiver>>();
  private receiveTimer?:number;
  private typingKey?: (event:KeyboardEvent)=>void;
  private typingClick?:()=>void;
  private elements = new Map<string,HTMLAudioElement>();
  private tracks = new Map<string,number>();
  private volumes = new Map<number,number>();
  private candidates: RTCIceCandidateInit[] = [];
  private active = false;
  private ready = false;
  private processingBlocked = false;
  private captureBlocked = false;
  private captureStatus = '';
  private captureSource?:MediaStreamAudioSourceNode;
  private captureInput?:AudioNode;
  private meterSources:MediaStreamAudioSourceNode[]=[];
  private microphoneRetry?:number;
  private microphoneTask?:Promise<void>;
  private microphoneRevision=0;
  private microphoneAttempts=0;
  private sender?:RTCRtpSender;
  private wake?:()=>void;
  private muted = false;
  private deafened = false;
  private away = false;
  private awayMessage = '';
  private confirmedAway = {enabled:false,text:''};
  private pendingAway?: string;
  private pressed = false;
  private mode: 'ptt'|'open' = 'open';
  private sink = '';
  private generation = 0;
  private handling = Promise.resolve();
  private intent?:{request:Event;device:string;listenOnly:boolean};
  private retryTimer?:number;
  private retryCount=0;
  private lastChannel?:number;
  private currentChannel?:number;
  private restoreChannel?:number;
  private restoring=false;
  private speech=false;
  private speechTimer?:number;
  private gated=false;
  private heartbeat?:number;
  private lastMessage=0;
  private failure?:string;
  constructor(private event: (event: Event) => void) {}
  send(event: Event) { if (this.socket?.readyState === WebSocket.OPEN) this.socket.send(JSON.stringify(event)); }
  async connect(request: Event, device: string, listenOnly: boolean) {
    this.close();this.intent={request:{...request},device,listenOnly};
    await this.open();
  }
  private async open() {
    const intent=this.intent;if(!intent)return;const {request,device,listenOnly}=intent;
    this.dispose();this.failure=undefined; const generation = this.generation;this.restoreChannel=this.lastChannel;this.restoring=false;
    try {
    this.context=new AudioContext({latencyHint:'interactive',sampleRate:48000});this.context.resume().catch(()=>{});
    let response:Response;try{response=await fetch('/api/rtc',{signal:AbortSignal.timeout(10000)});}catch{throw new Retryable('无法连接网关');}if(!response.ok){if(response.status===401||response.status===403)throw new Error('登录已过期，请重新登录');throw new Retryable('网关暂不可用');}
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
      const context=this.context!;
      const source=context.createMediaStreamSource(stream),gain=context.createGain(),destination=context.createMediaStreamDestination();
      this.captureSource=source;this.captureInput=gain;this.processed=destination.stream;gain.gain.value=this.settings.gain;gain.connect(destination);
      const voiceOnly=this.settings.voiceOnly===true;
      const noiseFailed=()=>{this.captureSource?.disconnect();this.noiseNode?.destroy();this.noiseNode=undefined;this.processingBlocked=true;gain.gain.value=0;this.update();this.event({type:'audio_processing',noise:'blocked',actual:this.stream?.getAudioTracks()[0].getSettings?.()});this.event({type:'notice',message:'本地降噪与发言检测不可用，已暂停麦克风发送；仍可收听。请重新连接重试，或断开后关闭降噪并使用按键发言。'});return 'blocked';};
      if(this.settings.noise==='rnnoise') {
        try {
          const {createNoise}=await import('./noise');const node=await createNoise(context,this.settings.keyboard,voiceOnly,this.settings.strength);
          if(generation!==this.generation){node.destroy();return;}
          this.noiseNode=node;this.gated=true;this.speech=node.speaking===true;node.onspeech=enabled=>{if(generation!==this.generation)return;window.clearTimeout(this.speechTimer);this.speechTimer=undefined;if(enabled){this.speech=true;this.update();}else this.speechTimer=window.setTimeout(()=>{if(generation!==this.generation)return;this.speech=false;this.update();},60);};this.captureInput=node.input;source.connect(node.input);node.output.connect(gain);processing=node.mode;
          if(node.warning)this.event({type:'notice',message:node.warning});
          node.onchange=message=>{if(generation!==this.generation)return;this.event({type:'audio_processing',noise:node.mode,voice_only:voiceOnly,actual:this.stream?.getAudioTracks()[0].getSettings?.()});this.event({type:'notice',message});};
          node.onerror=()=>{if(generation!==this.generation)return;noiseFailed();};
          this.typingKey=e=>{if(generation===this.generation&&this.mode==='open'&&this.settings.typing&&!e.ctrlKey&&!e.altKey&&!e.metaKey&&!e.isComposing)this.noiseNode?.typing();};window.addEventListener?.('keydown',this.typingKey);
          this.typingClick=()=>{if(generation===this.generation&&this.mode==='open'&&this.settings.typing)this.noiseNode?.typing();};window.addEventListener?.('pointerdown',this.typingClick);
        } catch {
          if(generation!==this.generation)return;
          processing=noiseFailed();
        }
      } else source.connect(gain);
      context.resume().catch(()=>{});sendStream=destination.stream;
    }
    if(generation!==this.generation){sendStream?.getTracks().forEach(t=>t.stop());return;}
    this.event({type:'audio_processing',noise:listenOnly?'listen':processing,voice_only:this.settings.noise==='rnnoise'&&this.settings.voiceOnly&&!this.processingBlocked,actual:this.stream?.getAudioTracks()[0].getSettings?.()});
    this.peer = new RTCPeerConnection({...config,bundlePolicy:'max-bundle'});
    const qualityPeer=this.peer;let reading=false;
    this.qualityTimer=window.setInterval(async()=>{if(reading||!qualityPeer.getStats)return;reading=true;try{const report=await qualityPeer.getStats();if(generation===this.generation)this.event({type:'quality',...connectionQuality(report)});}catch{if(generation===this.generation)this.event({type:'quality'});}finally{reading=false;}},2000);
    this.event({type:'audio_transport',state:'new'});this.peer.onconnectionstatechange=()=>{if(generation!==this.generation)return;this.event({type:'audio_transport',state:qualityPeer.connectionState});if(qualityPeer.connectionState==='failed')this.lost(true,'浏览器语音连接中断，正在自动重连。');};
    this.lastMessage=Date.now();this.heartbeat=window.setInterval(()=>{if(generation===this.generation&&Date.now()-this.lastMessage>45000)this.lost(true,'网关超过 45 秒没有响应，正在自动重连。');},5000);
    this.peer.onicecandidate=e=>{if(generation===this.generation&&e.candidate)this.send({type:'ice',candidate:e.candidate.toJSON()});};
    this.peer.ontrack=e=>{if(generation!==this.generation)return; const receiver=createReceiver(this.context!,e.track),audio = new Audio();this.receivers.set(e.track.id,receiver);audio.autoplay=true;audio.srcObject=receiver.stream;this.elements.set(e.track.id,audio);this.applyAudio();Promise.all([receiver.play(),audio.play()]).catch(()=>{if(generation===this.generation)this.event({type:'notice',message:'浏览器暂停了音频，请点击“恢复音频”。'});});e.track.onended=()=>{if(generation!==this.generation||this.receivers.get(e.track.id)!==receiver)return;audio.pause();audio.srcObject=null;this.elements.delete(e.track.id);receiver.destroy();this.receivers.delete(e.track.id);};};
    this.receiveTimer=window.setInterval(()=>{for(const receiver of this.receivers.values())receiver.tick(this.settings.receiveAutoGain);},50);
    if(sendStream){this.sendTrack=sendStream.getAudioTracks()[0].clone();this.sendTrack.enabled=false;this.sender=this.peer.addTrack(this.sendTrack,new MediaStream([this.sendTrack]));this.update();}
    this.watchCapture(stream,generation);if (stream&&sendStream) this.detect(stream,sendStream);
    const socket = new WebSocket(`${location.protocol==='https:'?'wss':'ws'}://${location.host}/api/connect`); this.socket=socket;
    socket.onopen=()=>{if(generation===this.generation){socket.send(JSON.stringify(request));this.send({type:'mute',muted:this.muted,deafened:this.deafened});this.sendTransmit();}};
    socket.onmessage=e=>{if(generation!==this.generation)return;this.lastMessage=Date.now();let message;try{message=JSON.parse(e.data);}catch{this.lost(false,'网关消息无效，连接已停止。');return;}if(message.type==='disconnected'){this.lost(message.retryable===true,typeof message.message==='string'?message.message:this.failure);return;}if(message.type==='error'){this.failure=String(message.message);this.event(message);return;}this.handling=this.handling.then(async()=>{
      if(generation!==this.generation)return;
      if(message.type==='state'){
        const first=!this.ready;
        if(first){this.ready=true;this.send({type:'mute',muted:this.muted,deafened:this.deafened});if(this.away)this.sendAway();this.sendTransmit();}
        const own=message.members.find((member:{id:number})=>member.id===message.own);
        if(!this.pendingAway){this.confirmedAway={enabled:own?.away===true,text:String(own?.awayMessage??'')};this.away=this.confirmedAway.enabled;this.awayMessage=this.confirmedAway.text;this.update();this.event({type:'away',enabled:this.away,text:this.awayMessage});}
        this.retryCount=0;this.event({type:'reconnecting',active:false});
        const channel=message.members.find((member:{id:number})=>member.id===message.own)?.channel;
        this.currentChannel=channel;
        if(this.restoreChannel!==undefined&&channel!==this.restoreChannel){
          if(!this.restoring){this.restoring=true;const target=message.channels.find((c:{id:number})=>c.id===this.restoreChannel);if(target)this.event({type:'restore_channel',channel:target.id,name:target.name});else{this.restoreChannel=undefined;this.event({type:'notice',message:'断线前的频道已不存在，已留在服务器默认频道。'});}}
        }else{this.restoreChannel=undefined;this.restoring=false;}
        if(this.restoreChannel===undefined)this.lastChannel=channel;
      }
      if(message.type==='result'&&message.id===this.pendingAway){this.pendingAway=undefined;if(!message.ok){this.away=this.confirmedAway.enabled;this.awayMessage=this.confirmedAway.text;}else {if(this.confirmedAway.enabled!==this.away)this.event({type:'afk_sound',enabled:this.away});this.confirmedAway={enabled:this.away,text:this.awayMessage};}this.update();this.event({type:'away',enabled:this.away,text:this.awayMessage});}
      const peer=this.peer; if(!peer)return;
      if(message.type==='offer'){await peer.setRemoteDescription(message.description);if(generation!==this.generation)return;for(const c of this.candidates)await peer.addIceCandidate(c);if(generation!==this.generation)return;this.candidates=[];const answer=await peer.createAnswer();if(generation!==this.generation)return;await peer.setLocalDescription(answer);if(generation!==this.generation)return;this.send({type:'answer',description:answer});}
      else if(message.type==='ice'){if(peer.remoteDescription)await peer.addIceCandidate(message.candidate);else this.candidates.push(message.candidate);}
      else if(message.type==='track'){this.tracks.set(message.track,message.client);this.applyAudio();}
      if(generation===this.generation)this.event(message);
    }).catch(()=>{if(generation!==this.generation)return;this.lost(true,'语音协商失败，正在自动重连；请检查浏览器及网络设置。');});};
    socket.onclose=()=>{if(generation===this.generation)this.lost(true,'网页与网关的连接中断，正在自动重连。');};
    socket.onerror=()=>{if(generation===this.generation)this.event({type:'error',message:'无法连接网关，请检查网络。'});};
    } catch(error){if(generation!==this.generation)return;this.dispose();if(error instanceof Retryable){this.schedule();return;}this.close();throw error;}
  }
  private watchCapture(stream:MediaStream|undefined,generation:number) {
    const context=this.context!,track=stream?.getAudioTracks()[0];
    if(this.wake){window.removeEventListener?.('pointerdown',this.wake);window.removeEventListener?.('keydown',this.wake);document.removeEventListener?.('visibilitychange',this.wake);}
    const check=()=>{
      if(generation!==this.generation||stream!==this.stream||this.captureStatus==='failed'||this.captureStatus==='recovering')return;
      const status=track?.readyState==='ended'?'ended':track?.muted?'muted':context.state==='running'?'running':'paused';
      this.captureBlocked=status!=='running';this.update();
      if(status!==this.captureStatus){this.captureStatus=status;if(this.ready)this.sendTransmit();this.event({type:'audio_capture',status,message:status==='running'?(track?'麦克风正常采集':'收听音频正常运行'):status==='muted'?'麦克风暂时没有提供声音，收听与频道连接保持':status==='ended'?'麦克风已断开，正在单独恢复；收听与频道连接保持':'浏览器暂停了音频，请点击“恢复音频”'});}
      if(status==='ended')void this.recoverMicrophone();
    };
    this.wake=()=>{if(generation!==this.generation||context.state==='closed')return;context.resume().then(check).catch(check);};
    context.onstatechange=()=>{check();if(generation===this.generation&&context.state!=='running')this.wake?.();};
    if(track){track.onmute=check;track.onunmute=check;track.onended=check;}
    window.addEventListener?.('pointerdown',this.wake);window.addEventListener?.('keydown',this.wake);document.addEventListener?.('visibilitychange',this.wake);
    check();this.wake();
  }
  private clearMeters(){if(this.timer)clearInterval(this.timer);this.timer=undefined;for(const source of this.meterSources)source.disconnect();this.meterSources=[];}
  private detect(input: MediaStream,output:MediaStream) {
    this.clearMeters();const generation=this.generation;
    const monitor=(stream:MediaStream)=>{const source=this.context!.createMediaStreamSource(stream),analyser=this.context!.createAnalyser();this.meterSources.push(source);analyser.fftSize=512;source.connect(analyser);const data=new Float32Array(analyser.fftSize);return()=>{analyser.getFloatTimeDomainData(data);return Math.min(1,Math.sqrt(data.reduce((sum,n)=>sum+n*n,0)/data.length)*8);};};
    const raw=monitor(input),processed=monitor(output);
    this.timer=window.setInterval(()=>{if(generation!==this.generation||input!==this.stream)return;try{this.event({type:'level',level:processed(),input_level:raw()});}catch{this.captureBlocked=true;this.update();if(this.ready)this.sendTransmit();this.event({type:'level',level:0,input_level:0});if(this.captureStatus!=='failed'){this.captureStatus='failed';this.event({type:'audio_capture',status:'failed',message:'麦克风采样暂时失败，正在单独恢复；收听与频道连接保持'});}void this.recoverMicrophone();}},30);
  }
  private recoverMicrophone():Promise<void> {
    if(this.microphoneTask)return this.microphoneTask;
    if(!this.intent||this.intent.listenOnly||!this.peer||!this.context||this.context.state==='closed')return Promise.resolve();
    window.clearTimeout(this.microphoneRetry);this.microphoneRetry=undefined;
    const generation=this.generation,revision=this.microphoneRevision,old=this.stream,device=this.intent.device;
    this.clearMeters();this.captureStatus='recovering';this.event({type:'level',level:0,input_level:0});this.event({type:'audio_capture',status:'recovering',message:'正在恢复麦克风；频道、聊天与收听保持'});
    this.captureBlocked=true;this.speech=false;window.clearTimeout(this.speechTimer);this.speechTimer=undefined;this.update();if(this.ready)this.sendTransmit();
    const task=(async()=>{
      let stream:MediaStream|undefined;
      try{
        let expired=false;let deadline:number|undefined;
        const request=navigator.mediaDevices.getUserMedia({audio:{deviceId:device?{exact:device}:undefined,echoCancellation:this.settings.echo,noiseSuppression:false,autoGainControl:this.settings.autoGain,channelCount:1}}).then(value=>{if(expired||generation!==this.generation||revision!==this.microphoneRevision)value.getTracks().forEach(t=>t.stop());return value;});
        try{stream=await Promise.race([request,new Promise<MediaStream>((_,reject)=>{deadline=window.setTimeout(()=>{expired=true;reject(new Error('采集设备响应超时'));},10000);})]);}finally{window.clearTimeout(deadline);}
        if(generation!==this.generation||revision!==this.microphoneRevision){stream.getTracks().forEach(t=>t.stop());return;}
        if(stream.getAudioTracks()[0]?.readyState==='ended')throw new Error('设备停止采集');
        if(this.captureInput&&this.processed){
          const source=this.context!.createMediaStreamSource(stream);source.connect(this.captureInput);this.captureSource?.disconnect();this.captureSource=source;
        }else{
          const track=stream.getAudioTracks()[0]?.clone();if(!track||!this.sender){track?.stop();throw new Error('麦克风发送轨道不可用');}
          track.enabled=false;
          try{await this.sender.replaceTrack(track);}catch(error){track.stop();throw error;}
          if(generation!==this.generation||revision!==this.microphoneRevision){track.stop();stream.getTracks().forEach(t=>t.stop());return;}
          this.sendTrack?.stop();this.sendTrack=track;
        }
        for(const track of old?.getTracks()??[]){track.onmute=null;track.onunmute=null;track.onended=null;track.stop();}
        this.stream=stream;this.microphoneAttempts=0;this.captureStatus='';this.watchCapture(stream,generation);this.detect(stream,this.processed??stream);
        this.event({type:'audio_processing',noise:this.processingBlocked?'blocked':this.noiseNode?.mode??this.settings.noise,voice_only:this.settings.voiceOnly&&!this.processingBlocked,actual:stream.getAudioTracks()[0].getSettings?.()});
      }catch(error){
        stream?.getTracks().forEach(t=>t.stop());if(generation!==this.generation||revision!==this.microphoneRevision)return;
        this.captureBlocked=true;this.captureStatus='failed';this.update();if(this.ready)this.sendTransmit();
        const denied=error instanceof DOMException&&['NotAllowedError','SecurityError'].includes(error.name);
        this.event({type:'audio_capture',status:'failed',message:denied?'麦克风权限被拒绝，仍保持频道与收听；授权后点击“恢复麦克风”':'麦克风暂时不可用，正在重试；频道与收听保持，可选择其他麦克风'});
        if(!denied)this.microphoneRetry=window.setTimeout(()=>{this.microphoneRetry=undefined;if(generation===this.generation&&revision===this.microphoneRevision)void this.recoverMicrophone();},Math.min(15000,2000*2**Math.min(this.microphoneAttempts++,3)));
      }
    })();
    this.microphoneTask=task;void task.finally(()=>{if(this.microphoneTask===task){this.microphoneTask=undefined;if(generation===this.generation&&revision!==this.microphoneRevision)void this.recoverMicrophone();}});return task;
  }
  inputDevice(device:string){if(!this.intent||this.intent.listenOnly)return;this.intent.device=device;this.microphoneRevision++;void this.recoverMicrophone();}
  private sendTransmit(){this.send({type:'transmit',enabled:this.active,pre_roll:this.mode==='open'&&this.gated&&!this.processingBlocked&&!this.captureBlocked&&!this.muted&&!this.away});}
  private update() {const capture=!!this.sendTrack&&!this.processingBlocked&&!this.captureBlocked&&!this.muted&&!this.away&&(this.mode==='open'||(this.pressed&&document.hasFocus()));if(this.sendTrack)this.sendTrack.enabled=capture;const active=capture&&(this.mode==='ptt'||!this.gated||this.speech);if(active!==this.active){this.active=active;if(this.ready)this.sendTransmit();this.event({type:'transmit',enabled:active});this.applyAudio();}}
  private sendAway(){const id=`away:${this.generation}:${Date.now()}:${Math.random()}`;this.pendingAway=id;this.send({type:'command',action:'away',id,enabled:this.away,text:this.awayMessage});}
  setAway(enabled:boolean,text=''){if(this.pendingAway)return;this.away=enabled;this.awayMessage=enabled?text:'';this.pressed=false;this.update();if(this.ready){this.sendTransmit();this.sendAway();}this.event({type:'away',enabled,text:this.awayMessage});}
  press(value:boolean){this.pressed=value;this.update();}
  setMode(mode:'ptt'|'open'){this.mode=mode;this.pressed=false;this.update();if(this.ready)this.sendTransmit();}
  configure(settings:AudioSettings){this.settings={...defaultAudioSettings,...settings};this.applyAudio();}
  outputVolume(value:number){this.settings.volume=Math.max(0,Math.min(1,value));this.applyAudio();}
  setMute(muted:boolean,deafened:boolean){this.muted=muted;this.deafened=deafened;this.update();this.applyAudio();this.send({type:'mute',muted,deafened});}
  volume(client:number,value:number){this.volumes.set(client,value);this.applyAudio();}
  async output(id:string){this.sink=id;for(const element of this.elements.values()){const audio=element as HTMLAudioElement & {setSinkId?:(id:string)=>Promise<void>};if(audio.setSinkId)await audio.setSinkId(id);}}
  private applyAudio(){for(const[id,audio]of this.elements){audio.muted=this.deafened;audio.volume=(this.volumes.get(this.tracks.get(id)??0)??1)*this.settings.volume*(this.active?1-this.settings.ducking:1);const element=audio as HTMLAudioElement & {setSinkId?:(id:string)=>Promise<void>};if(this.sink&&element.setSinkId)element.setSinkId(this.sink).catch(()=>{});}}
  resume(){if(['ended','failed','muted'].includes(this.captureStatus))void this.recoverMicrophone();if(this.wake)this.wake();else this.context?.resume().catch(()=>{});for(const receiver of this.receivers.values())receiver.play().catch(()=>this.event({type:'notice',message:'浏览器仍未允许接收音频，请检查网站声音权限。'}));for(const audio of this.elements.values())audio.play().catch(()=>this.event({type:'notice',message:'浏览器仍未允许播放，请检查网站声音权限。'}));}
  restoreFailed(){if(this.restoring){this.restoreChannel=undefined;this.restoring=false;this.lastChannel=this.currentChannel;}}
  private lost(retry:boolean,message=retry?'服务器连接中断，正在自动重连。':'服务器连接已停止，请检查提示后重新连接。'){this.dispose();this.event({type:'disconnected',message});if(retry)this.schedule();else{this.intent=undefined;this.lastChannel=undefined;this.event({type:'reconnecting',active:false});}}
  private schedule(){
    if(!this.intent||this.retryTimer!==undefined)return;
    const delay=Math.min(30000,1000*2**Math.min(this.retryCount++,5));
    this.event({type:'reconnecting',active:true,delay});
    this.retryTimer=window.setTimeout(async()=>{this.retryTimer=undefined;try{await this.open();}catch(error){this.event({type:'error',message:error instanceof Error?error.message:'重连失败'});this.event({type:'reconnecting',active:false});}},delay);
  }
  close(){this.away=false;this.awayMessage="";this.confirmedAway={enabled:false,text:""};this.event({type:"away",enabled:false,text:""});window.clearTimeout(this.retryTimer);this.retryTimer=undefined;this.intent=undefined;this.retryCount=0;this.lastChannel=undefined;this.currentChannel=undefined;this.restoreChannel=undefined;this.dispose();}
  private dispose(){window.clearTimeout(this.microphoneRetry);this.microphoneRetry=undefined;this.microphoneRevision++;this.microphoneTask=undefined;this.microphoneAttempts=0;this.clearMeters();this.captureSource?.disconnect();this.captureSource=undefined;this.captureInput=undefined;this.sender=undefined;if(this.typingClick)window.removeEventListener?.('pointerdown',this.typingClick);this.typingClick=undefined;if(this.typingKey)window.removeEventListener?.('keydown',this.typingKey);this.typingKey=undefined;if(this.receiveTimer)clearInterval(this.receiveTimer);this.receiveTimer=undefined;for(const receiver of this.receivers.values())receiver.destroy();this.receivers.clear();if(this.wake){window.removeEventListener?.('pointerdown',this.wake);window.removeEventListener?.('keydown',this.wake);document.removeEventListener?.('visibilitychange',this.wake);this.wake=undefined;}if(this.context)this.context.onstatechange=null;for(const track of this.stream?.getTracks()??[]){track.onmute=null;track.onunmute=null;track.onended=null;}this.pendingAway=undefined;this.captureBlocked=false;this.captureStatus='';window.clearTimeout(this.speechTimer);this.speechTimer=undefined;if(this.heartbeat)clearInterval(this.heartbeat);this.heartbeat=undefined;this.gated=false;this.speech=false;this.generation++;if(this.qualityTimer)clearInterval(this.qualityTimer);this.qualityTimer=undefined;this.handling=Promise.resolve();this.pressed=false;this.active=false;this.ready=false;this.processingBlocked=false;this.send({type:'disconnect'});const socket=this.socket;this.socket=undefined;if(socket){socket.onclose=null;socket.close();}this.peer?.close();this.peer=undefined;this.sendTrack?.stop();this.sendTrack=undefined;this.processed?.getTracks().forEach(t=>t.stop());this.processed=undefined;this.stream?.getTracks().forEach(t=>t.stop());this.stream=undefined;if(this.timer)clearInterval(this.timer);this.timer=undefined;this.noiseNode?.destroy();this.noiseNode=undefined;this.context?.close();this.context=undefined;for(const audio of this.elements.values()){audio.pause();audio.srcObject=null;}this.elements.clear();this.tracks.clear();this.volumes.clear();this.candidates=[];}
}
