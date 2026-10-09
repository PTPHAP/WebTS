import type {State} from './api';
import {ChannelText} from './ChannelText';
import type {TsImageState} from './ChannelText';
import {safeHref} from './ts-text';

export function ServerOverview({state,images,loadImage,imageLimit,back}:{state:State;images:Record<string,TsImageState>;loadImage:(url:string)=>void;imageLimit:number;back:()=>void}){
  const info=state.serverInfo,icon=info?images[`tsserver:icon:${info.icon}`]:undefined;
  const bannerKey=info?.banner?`tsserver:banner:${info.banner}`:'',banner=images[bannerKey];
  const bannerLink=safeHref(info?.bannerLink??''),buttonLink=safeHref(info?.buttonLink??'');
  return <article className="server-overview" aria-label="服务器主页"><header><span className="server-mark">{icon?.data?<img src={icon.data} alt="服务器图标"/>:'TS'}</span><div><span className="eyebrow">SERVER HOME</span><h2>{state.server}</h2></div><button className="secondary small" onClick={back}>返回频道</button></header>
    {info?.banner&&<section className="server-banner">{banner?.data?<img src={banner.data} alt="服务器横幅"/>:<><p>{banner?.error||'此服务器配置了外部横幅。点击后由网关安全读取，图片站点可见网关 IP。'}</p><button className="secondary" disabled={banner?.loading} onClick={()=>loadImage(bannerKey)}>{banner?.loading?'正在读取横幅…':banner?.error?'重试横幅':'显示服务器横幅'}</button></>}{bannerLink&&<a href={bannerLink} target="_blank" rel="noreferrer noopener">服务器主页 ↗</a>}</section>}
    <section className="server-introduction"><h3>服务器介绍</h3><ChannelText text={info?.welcome||'服务器尚未设置欢迎介绍。'} images={images} loadImage={loadImage} imageLimit={imageLimit}/>{info?.message&&info.message!==info.welcome&&<><h3>服务器公告</h3><ChannelText text={info.message} images={images} loadImage={loadImage} imageLimit={imageLimit}/></>}{buttonLink&&<a href={buttonLink} target="_blank" rel="noreferrer noopener">{info?.buttonLabel||'服务器提供的链接'} ↗</a>}</section>
    <dl className="server-information"><div><dt>当前可见成员</dt><dd>{state.members.length} 人</dd></div><div><dt>服务器名额</dt><dd>{info?.maxClients??'—'}</dd></div><div><dt>平台 / 版本</dt><dd>{info?`${info.platform} · ${info.version}`:'服务器未提供'}</dd></div><div><dt>服务器 UID</dt><dd className="uid">{info?.uid??'—'}</dd></div></dl>
    <small className="muted">介绍、图标和横幅来自当前 TeamSpeak 服务器；此页面不改变你的频道。</small>
  </article>;
}
