// Internal IDs only: never accept CSS, HTML, URLs or executable theme packages.
export const seasons=[
  {id:'classic',name:'经典',symbol:'◈',description:'熟悉的深浅色，专注每一句声音。',objects:''},
  {id:'spring',name:'新春 · 灯火',symbol:'❖',description:'灯笼挂起，梅花初绽，朋友们围坐迎新。',objects:'灯笼 · 梅花 · 鞭炮'},
  {id:'midautumn',name:'中秋 · 月泊',symbol:'☾',description:'一轮满月、一只玉兔，把远方也变成团圆。',objects:'满月 · 玉兔 · 月饼 · 桂枝'},
  {id:'winter',name:'圣诞 · 冬信',symbol:'✧',description:'树梢亮起星光，礼物和冬雪陪你等朋友。',objects:'圣诞树 · 礼物 · 雪花 · 拐杖糖'},
  {id:'halloween',name:'万圣 · 夜游',symbol:'✦',description:'南瓜戴上巫师帽，蝙蝠与烛光开启夜游。',objects:'南瓜灯 · 巫师帽 · 蝙蝠 · 烛灯'},
  {id:'newyear',name:'跨年 · 星序',symbol:'✺',description:'指针靠近零点，和朋友一起迎接漫天烟花。',objects:'零点时钟 · 烟花 · 星屑 · 城市剪影'},
] as const;
export type Season=typeof seasons[number]['id'];
export function readSeason(value:unknown):Season{return seasons.find(s=>s.id===value)?.id??'classic';}
export function effectiveSeason(site:unknown,preference:unknown):Season{return preference==='follow'?readSeason(site):'classic';}
