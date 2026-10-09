import {useEffect} from 'react';
import type {Home} from './SiteHome';
export function SiteIdentity({home}:{home:Home}){
  useEffect(()=>{document.title=`${home.site_name} · 让声音相聚`;const icon=document.querySelector<HTMLLinkElement>('link[rel="icon"]');if(icon){icon.type=home.site_icon?'image/png':'image/svg+xml';icon.href=home.site_icon||'/mark.svg';}},[home.site_name,home.site_icon]);
  return null;
}
export function SiteFooter({home,navigate}:{home:Home;navigate:(view:string)=>void}){
  return <footer className="global-footer"><div className="footer-links"><strong>{home.site_name}</strong><a href="/privacy" onClick={e=>{e.preventDefault();navigate('privacy');}}>隐私政策</a><a href="/terms" onClick={e=>{e.preventDefault();navigate('terms');}}>使用协议与免责声明</a><a href="https://github.com/PTPHAP/WebTS" target="_blank" rel="noreferrer">Powered by WebTS ↗</a></div>{home.footer_html&&<div className="custom-footer" dangerouslySetInnerHTML={{__html:home.footer_html}}/>}</footer>;
}
