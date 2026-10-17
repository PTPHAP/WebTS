import {useState} from 'react';

export function CommunityAvatar({row,account,self}:{row:{id:string;owner:number;name:string;avatar_hash?:string};account:number;self?:{avatar:string}}){
 const[failed,setFailed]=useState(''),mine=row.owner===account&&self;
 const hash=row.avatar_hash??'';
 const source=mine?(/^[A-Za-z0-9+/=]+$/.test(mine.avatar)&&mine.avatar.length<=174764?`data:image/png;base64,${mine.avatar}`:''):/^[a-f0-9]{64}$/.test(hash)&&/^[a-f0-9-]{36}$/i.test(row.id)?`/api/community/${encodeURIComponent(row.id)}/avatar?account=${account}&v=${hash}`:'';
 return <span className="community-avatar community-author-avatar" role="img" aria-label={`${row.name}的头像`}>{source&&source!==failed?<img src={source} alt="" loading="lazy" decoding="async" onError={()=>setFailed(source)}/>:<span aria-hidden="true">{Array.from(row.name)[0]||'友'}</span>}</span>;
}
