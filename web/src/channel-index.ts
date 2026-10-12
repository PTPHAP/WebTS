import type {Channel,Member} from './api';
import {channelSiblings} from './channel-order';
export function indexChannels(channels:Channel[],people:Member[]){
  const children=new Map<number,Channel[]>(),members=new Map<number,Member[]>();
  for(const c of channels){const group=children.get(c.parent)??[];group.push(c);children.set(c.parent,group);}
  for(const [parent,group] of children)children.set(parent,channelSiblings(group,parent));
  for(const m of people){const group=members.get(m.channel)??[];group.push(m);members.set(m.channel,group);}
  return {children,members};
}
