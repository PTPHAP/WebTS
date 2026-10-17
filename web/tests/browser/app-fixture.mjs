export async function appFixture(page,{loggedIn=true,watch=false,replies={}}={}){
  const now=Math.floor(Date.now()/1000),requests=[];
  const home={site_name:'WebTS',site_icon:'',footer_html:'',operator:'',contact:'',data_details:'',privacy_policy:'',terms:'',title:'今天，和朋友一起相聚。',subtitle:'本地界面验收，无真实用户数据。',image:'',announcements:[]};
  const data={
    '/api/site':home,
    '/api/community':{account:1,enabled:false,items:[],next:null,server_time:now},
    '/api/me':{user:loggedIn?{id:1,email:'navigation@example.invalid',is_admin:false}:null},
    '/api/profile':{display_name:'页面切换验收',avatar:''},
    '/api/preferences':{account:1,initialized:true,preferences:{}},
    '/api/identities':{identities:[]},
    '/api/servers':{servers:[{id:'fixture',name:'本地测试服务器'}],default_server:'fixture',allow_custom:false},
    '/api/notices':{account:1,items:[],unread:0},
    '/api/friends':{account:1,friends:[],requests:[],blocked:[],unread:0},
    '/api/friends/me':{id:1,code:'fixture',public_key:'',storage_enabled:false,retention_days:7,prekeys_left:0,share_presence:false,server_time:now},
    '/api/watch':watch?{enabled:true,config:{enabled:true,title:'本地播放器连续性验收',description:'隔离测试，不连接实际直播',url:'https://live.bilibili.com/7734200',cover:'',public:false,embed_enabled:true,ts_server:'fixture',ts_channel:0,score_mode:'none',stages:[]},revision:1,event_id:'fixture',platform:'bilibili',server_time:now,score:null}:{enabled:false},
    '/api/events':{account:1,items:[],server_time:now},
    ...replies,
  };
  await page.route('**/api/**',route=>{
    const path=new URL(route.request().url()).pathname;requests.push(path);
    return route.fulfill({status:Object.hasOwn(data,path)?200:404,contentType:'application/json',body:JSON.stringify(data[path]??{error:'隔离测试中未提供此接口'})});
  });
  return requests;
}
