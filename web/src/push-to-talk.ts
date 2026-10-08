export function validPttKey(code:string):boolean {
  return /^(Key[A-Z]|Digit[0-9]|Numpad[0-9]|F(?:[1-9]|1[0-2])|Space|(?:Shift|Control|Alt)(?:Left|Right))$/.test(code);
}
export function readPttKey():string {
  try{const code=localStorage.getItem('webts-ptt-key');return code&&validPttKey(code)?code:'KeyV';}catch{return 'KeyV';}
}
export function pttKeyLabel(code:string):string {
  const names:Record<string,string>={Space:'空格',ShiftLeft:'左 Shift',ShiftRight:'右 Shift',ControlLeft:'左 Ctrl',ControlRight:'右 Ctrl',AltLeft:'左 Alt',AltRight:'右 Alt'};
  return names[code]??code.replace(/^Key|^Digit/,'').replace(/^Numpad/,'小键盘 ');
}
export function bindPushToTalk(window:Window,document:Document,code:string,press:(enabled:boolean)=>void):()=>void {
  let held=false;
  const editable=(target:EventTarget|null)=>target instanceof Element&&!!target.closest('input,textarea,select,button,[contenteditable]:not([contenteditable="false"]),[role="textbox"]');
  const release=()=>{held=false;press(false);};
  const key=(event:KeyboardEvent)=>{
    if(event.code!==code)return;
    if(event.type==='keyup'){if(held)release();return;}
    if(event.repeat||event.isComposing||document.hidden||!document.hasFocus()||editable(event.target)||event.metaKey||event.ctrlKey&&!code.startsWith('Control')||event.altKey&&!code.startsWith('Alt'))return;
    event.preventDefault();held=true;press(true);
  };
  const focus=(event:FocusEvent)=>{if(editable(event.target))release();};
  window.addEventListener('keydown',key);window.addEventListener('keyup',key);window.addEventListener('blur',release);
  document.addEventListener('visibilitychange',release);document.addEventListener('focusin',focus);
  return()=>{release();window.removeEventListener('keydown',key);window.removeEventListener('keyup',key);window.removeEventListener('blur',release);document.removeEventListener('visibilitychange',release);document.removeEventListener('focusin',focus);};
}
