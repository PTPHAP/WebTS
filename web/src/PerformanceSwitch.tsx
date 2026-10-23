export function PerformanceSwitch({enabled,onChange}:{enabled:boolean;onChange:(enabled:boolean)=>void}) {
  return <label className="low-resource-toggle" title="减少界面特效和后台电平刷新；语音处理保持原样">
    <span>低占用</span>
    <input type="checkbox" role="switch" aria-label="低占用模式" aria-checked={enabled} checked={enabled} onChange={event=>onChange(event.currentTarget.checked)}/>
    <span className="low-resource-state">{enabled?'已开启':'已关闭'}</span>
  </label>;
}
