export type ChannelFolds=Record<string,number[]>;
export function foldedChannelMembers<T extends {channel:number}>(channels:{id:number;parent:number}[],members:T[],channel:number):T[] {
  if(!channels.some(c=>c.id===channel))return [];
  const children=new Map<number,number[]>();
  for(const c of channels){const ids=children.get(c.parent)??[];ids.push(c.id);children.set(c.parent,ids);}
  const visible=new Set([channel]);let level=[channel];
  for(let depth=0;depth<20&&level.length;depth++){
    const next:number[]=[];
    for(const id of level)for(const child of children.get(id)??[])if(!visible.has(child)){visible.add(child);next.push(child);}
    level=next;
  }
  return members.filter(m=>visible.has(m.channel));
}
// Keep account preferences bounded; channel IDs belong to one server UID only.
export function toggleChannelFold(before:ChannelFolds,server:string,channel:number):ChannelFolds {
  const current=Object.hasOwn(before,server)?before[server]:[];
  const next=current.includes(channel)?current.filter(id=>id!==channel):[...current,channel].slice(-128);
  const entries=Object.entries(before).filter(([key])=>key!==server);
  if(next.length)entries.push([server,next]);
  return Object.fromEntries(entries.slice(-4));
}
