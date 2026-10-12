import {diagnosticReport,voiceDiagnosis,type DiagnosticInput} from './voice-diagnostics';
export function VoiceDiagnostics({value}:{value:DiagnosticInput}){
  function download(){const url=URL.createObjectURL(new Blob([JSON.stringify(diagnosticReport(value),null,2)],{type:'application/json'}));const a=document.createElement('a');a.href=url;a.download='webts-diagnostics.json';a.click();setTimeout(()=>URL.revokeObjectURL(url),1000);}
  return <section className="voice-diagnostics"><h3>语音诊断</h3>{voiceDiagnosis(value).map(row=><article key={row.title}><strong>{row.title}</strong><p>{row.help}</p></article>)}<p className="muted">浏览器链路：{value.quality.route==='relay'?'TURN 中继':value.quality.route==='direct'?'直连':'尚未报告'} · 接收 {value.quality.received??'—'} / 发送 {value.quality.sent??'—'} 包</p><button className="secondary full" onClick={download}>下载脱敏诊断报告</button><small>报告仅含开关状态、加密套件和数值统计，不含账号、IP、设备编号、密钥或聊天内容。请预览后再分享。</small></section>;
}
