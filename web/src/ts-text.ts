export type TextPart={text:string;style?:'b'|'i'|'u';href?:string};
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
