export type TextPart={text:string;style?:'b'|'i'|'u';href?:string};
export type TsNode={text:string}|{tag:string;param?:string;children:TsNode[]};
const tags=new Set(['b','i','u','s','sup','sub','color','size','left','center','right','url','img','hr','list','li','table','tr','th','td']);
export function safeHref(value:string):string|undefined {
  if(value.length>2048||/[\x00-\x20\x7f]/.test(value))return;
  try{const url=new URL(value);if(['https:','http:','ts3server:'].includes(url.protocol)&&!url.username&&!url.password)return url.href;}catch{}
}
export function safeColor(value:string|undefined):string|undefined {return value&&/^(#[\da-f]{3,4}|#[\da-f]{6}|#[\da-f]{8}|[a-z]{1,20})$/i.test(value)?value:undefined;}
export function safeSize(value:string|undefined):number|undefined {
  if(!value||!/^([+-][1-7]|[1-9]\d?)$/.test(value))return;
  const size=/^[+-]/.test(value)?16+Number(value)*2:Number(value);return size>=8&&size<=48?size:undefined;
}
export function plainTsText(nodes:TsNode[]):string {return nodes.map(n=>'text'in n?n.text:plainTsText(n.children)).join('');}
export function parseTsText(value:string):TsNode[] {
  const text=value.slice(0,16384),root:TsNode[]=[],stack:{tag:string;children:TsNode[]}[]=[{tag:'',children:root}];
  const append=(value:string)=>{if(value)stack.at(-1)!.children.push({text:value});};
  const tokens=/\[(\/?)([a-z]+|\*)(?:=([^\]\r\n]{0,2048}))?\]/gi;let end=0,count=0;
  for(const token of text.matchAll(tokens)){
    if(++count>2048)break;append(text.slice(end,token.index));end=token.index!+token[0].length;
    const tag=token[2]==='*'?'li':token[2].toLowerCase();
    if(!tags.has(tag)){append(token[0]);continue;}
    if(token[1]){
      if(tag==='list'&&stack.at(-1)?.tag==='li')stack.pop();
      if(stack.length>1&&stack.at(-1)!.tag===tag)stack.pop();else append(token[0]);
    }else{
      if(tag==='li'){let list=stack.length-1;while(list>0&&stack[list].tag!=='list')list--;if(!list){append(token[0]);continue;}stack.length=list+1;}
      if(stack.length>=24){append(token[0]);continue;}
      const param=token[3]?.replace(/^(["'])(.*)\1$/,'$2');const node={tag,param,children:[] as TsNode[]};stack.at(-1)!.children.push(node);if(tag!=='hr')stack.push(node);
    }
  }
  append(text.slice(end));return root;
}
export function channelSpacer(name:string):{align:'left'|'center'|'right';text:string;repeat:boolean}|undefined {
  const match=/^\[([clr*]?)spacer[^\]\r\n]{0,64}\](.*)$/i.exec(name);if(!match)return;
  const mode=match[1].toLowerCase(),text=match[2];return {align:mode==='c'?'center':mode==='r'?'right':'left',text,repeat:mode==='*'||(!mode&&['---','___','...','-.-','--.'].includes(text))};
}
export function tsText(value:string):TextPart[] {
  const text=value.slice(0,16384),parts:TextPart[]=[];
  const expression=/\[(b|i|u|url)(?:=([^\]\r\n]{1,2048}))?\]([^]*?)\[\/\1\]/gi;
  let end=0;
  for(const match of text.matchAll(expression)) {
    parts.push({text:text.slice(end,match.index)});
    const tag=match[1].toLowerCase();
    if(tag==='url') {
      const address=match[2]??match[3];let href:string|undefined;
      try {const url=new URL(address);if(url.protocol==='https:'||url.protocol==='http:')href=url.href;}catch{}
      parts.push({text:match[3],href});
    }else parts.push({text:match[3],style:tag as 'b'|'i'|'u'});
    end=match.index!+match[0].length;
  }
  parts.push({text:text.slice(end)});return parts;
}
