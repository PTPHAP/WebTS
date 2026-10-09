import type {Home} from './SiteHome';
function inline(text:string,siteName:string){const named=(value:string)=>value.replaceAll('{{site_name}}',siteName);return text.split(/(\*\*[^*\n]{1,500}\*\*|\[[^\]\n]{1,200}\]\([^\)\n]{1,2048}\))/g).map((part,i)=>{if(part.startsWith('**')&&part.endsWith('**'))return <strong key={i}>{named(part.slice(2,-2))}</strong>;const link=part.match(/^\[([^\]]+)\]\(([^)]+)\)$/);if(link){let safe=false;try{const url=new URL(link[2]);safe=url.protocol==='https:'&&!url.username&&!url.password;}catch{}return safe?<a key={i} href={link[2]} target="_blank" rel="noreferrer">{named(link[1])}</a>:named(link[1]);}return named(part);});}
function blocks(text:string,siteName:string){return text.split(/\n\s*\n/).map((block,i)=>{
  if(block.startsWith('# '))return null;if(block.startsWith('## '))return <h2 key={i}>{inline(block.slice(3),siteName)}</h2>;
  if(block.startsWith('|')){const rows=block.split('\n').filter(row=>!/^\|[\s:|-]+\|$/.test(row)).map(row=>row.split('|').slice(1,-1).map(cell=>cell.trim()));return <div className="legal-table" key={i}><table><thead><tr>{rows[0]?.map((cell,n)=><th key={n}>{inline(cell,siteName)}</th>)}</tr></thead><tbody>{rows.slice(1).map((row,n)=><tr key={n}>{row.map((cell,j)=><td key={j}>{inline(cell,siteName)}</td>)}</tr>)}</tbody></table></div>;}
  if(block.startsWith('- '))return <ul key={i}>{block.split('\n').map((line,n)=><li key={n}>{inline(line.replace(/^- /,''),siteName)}</li>)}</ul>;
  return <p key={i}>{inline(block,siteName)}</p>;
});}
export function Legal({home,kind}:{home:Home;kind:'privacy'|'terms'}){
  const text=kind==='privacy'?home.privacy_policy:home.terms;
  return <main className="legal-page"><span className="eyebrow">{home.site_name} · 站点协议</span><h1>{kind==='privacy'?'隐私政策':'使用协议与免责声明'}</h1><aside className="legal-operator"><p><strong>站点运营者</strong> {home.operator||'尚未公示，请联系本站管理员确认'}</p><p><strong>联系与数据处理申请</strong> {home.contact||'尚未填写，请通过站点公告联系管理员'}</p>{home.data_details&&<p className="legal-details">{home.data_details}</p>}</aside><p className="boundary">语音经网关转接，网关可接触语音明文；身份私钥由服务器加密托管。本站管理员身份不授予额外 TS 权限。</p><article className="legal-copy">{text?blocks(text,home.site_name):<p>正在读取协议。若暂时无法加载，请刷新后再进行账号或身份操作。</p>}</article></main>;
}
