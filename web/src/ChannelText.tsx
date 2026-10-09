import {useEffect,useState} from 'react';
import type {ReactNode} from 'react';
import {parseTsText,plainTsText,safeColor,safeHref,safeSize} from './ts-text';
import type {TsNode} from './ts-text';
export type TsImageState={data?:string;error?:string;loading?:boolean};
function TsImage({url,state,load}:{url:string;state?:TsImageState;load?:()=>void}) {
  const[failed,setFailed]=useState(false);
  useEffect(()=>{if(url.startsWith('ts3image://')&&load&&!state)load();},[url,state,load]);
  if(state?.data&&!failed)return <img className="ts-image" src={state.data} alt="频道配图" onError={()=>setFailed(true)}/>;
  const href=safeHref(url);
  if(!url.startsWith('ts3image://'))return href?<a href={href} target="_blank" rel="noreferrer noopener">打开外部图片（外部站点可见你的 IP）</a>:<span>[img]{url}[/img]</span>;
  return <span className="ts-image-placeholder">{state?.error|| (failed?'图片无法显示':state?.loading?'正在读取频道图片…':'TeamSpeak 频道图片')}<button className="text-button" disabled={!load||state?.loading} onClick={load}>{state?.error?'重试':'加载图片'}</button></span>;
}
export function ChannelText({text,images={},loadImage,simple=false}:{text:string;images?:Record<string,TsImageState>;loadImage?:(url:string)=>void;simple?:boolean}) {
  let imageCount=0;
  const render=(nodes:TsNode[],interactive=true):ReactNode=>nodes.map((node,i)=>{
    if('text'in node)return <span key={i}>{node.text}</span>;
    const {tag,param,children}=node,content=tag==='url'||tag==='img'?null:render(children,interactive);
    if(simple&&!['b','i','u','s','sup','sub','color','url'].includes(tag))return <span key={i}>[{tag}{param?'='+param:''}]{plainTsText(children)}{tag==='hr'?'':`[/${tag}]`}</span>;
    switch(tag){
      case 'b':return <strong key={i}>{content}</strong>;case 'i':return <em key={i}>{content}</em>;case 'u':return <u key={i}>{content}</u>;case 's':return <s key={i}>{content}</s>;case 'sup':return <sup key={i}>{content}</sup>;case 'sub':return <sub key={i}>{content}</sub>;
      case 'color':return <span key={i} style={{color:safeColor(param)}}>{content}</span>;
      case 'size':return <span key={i} style={{fontSize:safeSize(param)}}>{content}</span>;
      case 'left':case 'center':case 'right':return <span key={i} className="ts-align" style={{textAlign:tag}}>{content}</span>;
      case 'url':{const href=safeHref(param??plainTsText(children));return href&&interactive?<a key={i} href={href} target="_blank" rel="noreferrer noopener">{render(children,false)}</a>:<span key={i}>{render(children,false)}</span>;}
      case 'img':{const url=plainTsText(children).trim();return interactive&&++imageCount<=8?<TsImage key={url+':'+i} url={url} state={images[url]} load={loadImage?()=>loadImage(url):undefined}/>:<span key={i}>[img]{url}[/img]</span>;}
      case 'hr':return <hr key={i}/>;
      case 'list':return <span key={i} className={`ts-list ${param?'ordered':''}`} style={{listStyleType:param==='A'?'upper-alpha':param==='a'?'lower-alpha':param?'decimal':'disc'}}>{content}</span>;
      case 'li':return <span key={i} className="ts-list-item">{content}</span>;
      case 'table':case 'tr':case 'td':case 'th':return <span key={i} className={`ts-${tag}`}>{content}</span>;
      default:return <span key={i}>{content}</span>;
    }
  });
  return <div className="ts-text">{render(parseTsText(text))}</div>;
}
