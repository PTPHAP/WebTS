export const panelIds=['channels','chat','members'] as const;
export const phoneLayoutQuery='(hover:none) and (pointer:coarse)';
export type PanelId=typeof panelIds[number];
export type Layout={order:PanelId[];widths:Record<PanelId,number>};
export const defaultLayout:Layout={order:[...panelIds],widths:{channels:30,chat:48,members:22}};
export function readPanelOrder(value:unknown):PanelId[] {
  return Array.isArray(value)&&value.length===3&&new Set(value).size===3&&value.every(id=>panelIds.includes(id))?[...value]:[...panelIds];
}
export function compactLayout(_saved:unknown,phone:boolean,device?:Pick<Navigator,'userAgent'|'maxTouchPoints'>&{userAgentData?:{mobile?:boolean}}):boolean {
  const agent=device?.userAgent??'';
  return phone||device?.userAgentData?.mobile===true||/Android|iPhone|iPad|iPod|Mobile/i.test(agent)||(device?.maxTouchPoints??0)>1&&/Macintosh/i.test(agent);
}
export function minimumWorkspaceWidth(layout:Layout):number {
  return Math.ceil(Math.max(280/layout.widths.channels,360/layout.widths.chat,240/layout.widths.members)*100);
}
export function readLayout(value:unknown):Layout {
  const fallback=()=>({order:[...defaultLayout.order],widths:{...defaultLayout.widths}});
  if(!value||typeof value!=='object')return fallback();
  const v=value as Partial<Layout>;
  if(!Array.isArray(v.order)||v.order.length!==3||new Set(v.order).size!==3||!v.order.every(id=>panelIds.includes(id)))return fallback();
  const widths=v.widths;
  if(!widths||!panelIds.every(id=>Number.isFinite(widths[id])&&widths[id]>=(id==='chat'?30:15)&&widths[id]<=70)||Math.abs(panelIds.reduce((n,id)=>n+widths[id],0)-100)>.01)return fallback();
  if(v.order.join(',')===panelIds.join(',')&&widths.channels===22&&widths.chat===54&&widths.members===24)return fallback();
  return {order:[...v.order],widths:{...widths}};
}
export function reorderPanels(layout:Layout,from:PanelId,to:PanelId):Layout {
  const order=layout.order.filter(id=>id!==from);order.splice(layout.order.indexOf(to),0,from);return {...layout,order};
}
export function resizePanels(layout:Layout,index:number,delta:number):Layout {
  const a=layout.order[index],b=layout.order[index+1];if(!a||!b)return layout;
  const lower=(a==='chat'?30:15)-layout.widths[a],upper=layout.widths[b]-(b==='chat'?30:15);
  const change=Math.max(lower,Math.min(upper,delta));return {...layout,widths:{...layout.widths,[a]:layout.widths[a]+change,[b]:layout.widths[b]-change}};
}
