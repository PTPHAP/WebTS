import type {Channel} from './api';

// TeamSpeak order is the preceding sibling's ID; zero marks the first sibling.
export function channelSiblings(channels:readonly Channel[],parent:number):Channel[] {
  const siblings=channels.filter(c=>c.parent===parent),ids=new Set(siblings.map(c=>c.id));
  const after=new Map(siblings.map(c=>[c.order,c])),seen=new Set<number>(),ordered:Channel[]=[];
  const append=(first:Channel)=>{let channel:Channel|undefined=first;while(channel&&!seen.has(channel.id)){seen.add(channel.id);ordered.push(channel);channel=after.get(channel.id);}};
  for(const channel of siblings)if(channel.order===0||!ids.has(channel.order))append(channel);
  // Partial or inconsistent snapshots must still display every visible sibling.
  for(const channel of siblings)append(channel);
  return ordered;
}
