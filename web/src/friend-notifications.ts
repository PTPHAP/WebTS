export type FriendEvent={kind:'message'|'request'|'accepted';peer?:number};
type Relation={id:number;status:number;requester:number};
export type FriendSnapshot={latest:number;relations:Relation[]};
export function friendEvents(previous:FriendSnapshot|undefined,next:FriendSnapshot,own:number):FriendEvent[]{
  if(!previous)return [];
  const old=new Map(previous.relations.map(f=>[f.id,f])),events:FriendEvent[]=[];
  if(next.latest>previous.latest)events.push({kind:'message'});
  for(const friend of next.relations){const before=old.get(friend.id);
    if(friend.status===0&&friend.requester!==own&&before?.status!==0)events.push({kind:'request',peer:friend.id});
    if(friend.status===1&&before?.status===0&&friend.requester===own)events.push({kind:'accepted',peer:friend.id});
  }
  return events;
}
