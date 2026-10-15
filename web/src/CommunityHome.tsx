import {useEffect,useState} from 'react';
import {api} from './api';
import type {Home} from './SiteHome';
import type {Favorite} from './favorites';
import './community-home.css';

type Friend={id:number;name:string;avatar_hash?:string;online?:boolean;status:number;blocked_by:number};
type FriendsReply={account:number;friends:Friend[]};
type Data={friends:Friend[];friendsError:boolean;loading:boolean};
function Avatar({friend}:{friend:Friend}){
  const[failed,setFailed]=useState(''),hash=friend.avatar_hash??'';
  return <span className="community-avatar">{/^[a-f0-9]{64}$/.test(hash)&&hash!==failed?<img src={`/api/friends/${friend.id}/avatar?v=${hash}`} alt="" loading="lazy" onError={()=>setFailed(hash)}/>:Array.from(friend.name||'友')[0]}</span>;
}
export function CommunityHome({account,name,home,servers,favorites,connected,reconnecting,channel,enter,security,friends,join}:{account:number;name:string;home:Home;servers:{id:string;name:string}[];favorites:Favorite[];connected:boolean;reconnecting:boolean;channel:string;enter:()=>void;security:()=>void;friends:()=>void;join:(value:Favorite)=>void}){
  const[data,setData]=useState<Data>({friends:[],friendsError:false,loading:true});
  const[revision,setRevision]=useState(0);
  useEffect(()=>{
    let alive=true,busy=false,controller:AbortController|undefined;
    async function refresh(){
      if(busy)return;busy=true;controller=new AbortController();
      const timeout=setTimeout(()=>controller?.abort(),10000);
      try{
        const people=await api<FriendsReply>('/friends',undefined,undefined,controller.signal);
        if(!alive)return;if(people.account!==account||!Array.isArray(people.friends))throw Error('账号已变化');
        setData({friends:people.friends.filter(friend=>friend.status===1&&!friend.blocked_by),friendsError:false,loading:false});
      }catch{if(alive)setData({friends:[],friendsError:true,loading:false});
      }finally{clearTimeout(timeout);controller=undefined;busy=false;}
    }
    void refresh();
    const visible=()=>{if(!document.hidden)void refresh();};
    const timer=setInterval(visible,30000);document.addEventListener('visibilitychange',visible);
    return()=>{alive=false;controller?.abort();clearInterval(timer);document.removeEventListener('visibilitychange',visible);};
  },[account,revision]);
  const online=data.friends.filter(friend=>friend.online===true).length;
  const people=[...data.friends].sort((a,b)=>Number(b.online===true)-Number(a.online===true)||a.id-b.id).slice(0,6);
  const announcements=home.announcements.filter(item=>item.enabled);
  return <main className="community-home">
    <section className={`community-welcome ${home.image?'has-image':''}`}>
      {home.image&&<figure className="community-image-card"><img className="home-display-image" src={home.image} alt="站点展示图片"/></figure>}
      <div className="community-welcome-copy"><span className="eyebrow">YOUR COMMUNITY · {home.site_name}</span><p className="community-greeting">你好，{name}</p><h1>{home.title}</h1><p>{home.subtitle}</p><div className="community-welcome-actions"><button className="primary" onClick={enter}>{connected?'返回语音空间':reconnecting?'查看重连进度':'进入语音空间'} ↗</button><button className="secondary" onClick={security}>账号安全</button></div></div>
      <aside className="community-now"><span className={`dot ${connected?'online':''}`}/><strong>{connected?'语音连接保持中':reconnecting?'正在恢复连接':'准备好，就一起出发'}</strong><p>{connected?(channel||'点击返回当前频道'):reconnecting?'恢复网络后自动重试，保留断线前频道。':'选择你的身份，加入熟悉的 TeamSpeak 频道。'}</p><small>切换首页不会主动结束通话</small></aside>
    </section>
    <div className="community-home-grid">
      <section className="community-section" aria-label="我的频道收藏"><header><div><span className="eyebrow">FAMILIAR PLACES</span><h2>熟悉的频道</h2></div><button className="text-button" onClick={enter}>语音空间 ↗</button></header>
        {favorites.length?<div className="community-favorites">{favorites.map(value=>{const server=servers.find(server=>server.id===value.server);return <button className="community-favorite" key={`${value.server}:${value.uid}:${value.channel}`} disabled={!server} onClick={()=>join(value)}><span aria-hidden="true">#</span><div><strong>{value.name}</strong><small>{server?.name??'服务器已移除'}</small></div><span aria-hidden="true">↗</span></button>;})}</div>:<div className="community-empty"><h3>常去的地方，一步就到</h3><p>在频道菜单中收藏频道，它会随账号显示在这里。</p><button className="secondary" onClick={enter}>去看看频道</button></div>}
      </section>
      <section className="community-section community-bulletin" aria-label="社区公告"><header><div><span className="eyebrow">COMMUNITY NOTES</span><h2>这里的新消息</h2></div><span className="community-caption">站点公告</span></header>{announcements.length?<div className="community-announcements">{announcements.map(item=><article key={item.id}>{item.image&&<img className="announcement-image" src={item.image} alt="公告配图" loading="lazy"/>}<div><h3>{item.title}</h3><p>{item.text}</p></div></article>)}</div>:<p className="community-empty">站长发布的公告将在这里展示。</p>}</section>
      <section className="community-section" aria-label="我的好友"><header><div><span className="eyebrow">YOUR PEOPLE</span><h2>和熟悉的人相聚</h2></div><button className="text-button" onClick={friends}>好友 ↗</button></header>
        {data.loading?<p className="community-empty" role="status">正在读取好友…</p>:data.friendsError?<div className="community-empty" role="status"><p>暂时无法读取好友，请检查网络或登录状态。</p><button className="secondary small" onClick={()=>setRevision(v=>v+1)}>重新读取</button></div>:<><p className="community-friend-summary">{data.friends.length} 位好友 · {online} 位分享了在线状态</p>{people.length?<div className="community-people">{people.map(friend=><button key={friend.id} onClick={friends}><Avatar friend={friend}/><div><strong>{friend.name||`WebTS 用户 ${friend.id}`}</strong><small><span className={`dot ${friend.online===true?'online':''}`}/>{friend.online===true?'在线':friend.online===false?'离线':'状态已隐藏'}</small></div><span aria-hidden="true">↗</span></button>)}</div>:<div className="community-empty"><p>添加好友，邀请他们一起聊天、开黑或看比赛。</p><button className="secondary" onClick={friends}>添加好友</button></div>}</>}
      </section>
    </div>
  </main>;
}
