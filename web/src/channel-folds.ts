export type ChannelFolds=Record<string,number[]>;
// Keep account preferences bounded; channel IDs belong to one server UID only.
export function toggleChannelFold(before:ChannelFolds,server:string,channel:number):ChannelFolds {
  const current=Object.hasOwn(before,server)?before[server]:[];
  const next=current.includes(channel)?current.filter(id=>id!==channel):[...current,channel].slice(-128);
  const entries=Object.entries(before).filter(([key])=>key!==server);
  if(next.length)entries.push([server,next]);
  return Object.fromEntries(entries.slice(-4));
}
