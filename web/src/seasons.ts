// Internal IDs only: never accept CSS, HTML, URLs or executable theme packages.
export const seasons=[
  {id:'classic',name:'经典',symbol:'◈',description:'熟悉的深浅色，专注每一句声音。'},
  {id:'spring',name:'新春 · 灯火',symbol:'❖',description:'朱红暖色，给相聚添一点年味。'},
  {id:'midautumn',name:'中秋 · 月泊',symbol:'☾',description:'月色蓝金，隔着距离也能同频。'},
  {id:'winter',name:'圣诞 · 冬信',symbol:'✧',description:'松绿与暖光，留一盏灯等朋友。'},
  {id:'halloween',name:'万圣 · 夜游',symbol:'✦',description:'紫色暮光，今晚一起开启下一局。'},
  {id:'newyear',name:'跨年 · 星序',symbol:'✺',description:'星蓝与微光，迎接下一次相聚。'},
] as const;
export type Season=typeof seasons[number]['id'];
export function readSeason(value:unknown):Season{return seasons.find(s=>s.id===value)?.id??'classic';}
export function effectiveSeason(site:unknown,preference:unknown):Season{return preference==='follow'?readSeason(site):'classic';}
