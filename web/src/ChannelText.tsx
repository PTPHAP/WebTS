import {tsText} from './ts-text';
export function ChannelText({text}:{text:string}) {
  return <div className="ts-text">{tsText(text).map((part,i)=>part.href?<a key={i} href={part.href} target="_blank" rel="noreferrer noopener">{part.text}</a>:part.style==='b'?<strong key={i}>{part.text}</strong>:part.style==='i'?<em key={i}>{part.text}</em>:part.style==='u'?<u key={i}>{part.text}</u>:<span key={i}>{part.text}</span>)}</div>;
}
