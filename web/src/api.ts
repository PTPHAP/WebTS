export async function api<T>(path: string, body?: unknown, method?: string): Promise<T> {
  const response = await fetch(`/api${path}`, { method: method ?? (body === undefined ? 'GET' : 'POST'), credentials: 'same-origin', headers: body === undefined ? {} : {'Content-Type':'application/json'}, body: body === undefined ? undefined : JSON.stringify(body) });
  if (!response.ok) { const error = await response.json().catch(() => null); throw new Error(error?.error ?? `请求失败 (${response.status})`); }
  return response.json();
}
export type Identity = {id:string; name:string; uid:string; is_default:boolean};
export type Member = {id:number; channel:number; name:string; uid?:string; avatarHash:string; muted:boolean; deafened:boolean; away?:boolean; awayMessage?:string; description:string; talkPower:number; serverGroups:number[]; channelGroup:number};
export type Channel = {id:number; parent:number; order:number; name:string; topic?:string; description?:string; password:boolean; codec?:number;quality?:number;kind?:string;maxClients?:number;maxFamilyClients?:number;neededTalkPower?:number};
export type State = {server:string; own:number; canSpeak:boolean; channels:Channel[]; members:Member[]};
