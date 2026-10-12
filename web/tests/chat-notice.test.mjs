import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import vm from 'node:vm';
import ts from 'typescript';
const code=ts.transpileModule(await readFile(new URL('../src/ChatNotice.tsx',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.CommonJS,jsx:ts.JsxEmit.React}}).outputText;
function render(message){
 let timer,cleared=false,closed=false,delay;
 const context={exports:{},React:{createElement:(type,props,...children)=>({type,props,children})},require:()=>({useRef:v=>({current:v}),useEffect:fn=>{context.cleanup=fn();}}),setTimeout(fn,ms){timer=fn;delay=ms;return 1;},clearTimeout(){cleared=true;}};
 vm.runInContext(code,vm.createContext(context));const tree=context.exports.ChatNotice({message,dismiss:()=>closed=true});
 return {tree,get delay(){return delay;},fire:()=>timer?.(),cleanup:()=>context.cleanup?.(),get closed(){return closed;},get cleared(){return cleared;}};
}
test('delivery confirmations disappear without reserving chat space and cancel timers when replaced',()=>{
 const notice=render('已加密保存至对象存储 · 对方上线可读取');assert.equal(notice.delay,4500);notice.fire();assert.equal(notice.closed,true);notice.cleanup();assert.equal(notice.cleared,true);
 assert.equal(render('').tree,null);
});
test('site success confirmations dismiss automatically while failures stay visible',()=>{
 for(const message of ['账号头像已同步到当前 TeamSpeak 身份。','频道操作已完成','成员已移动。','已恢复频道发言。']){
  const notice=render(message);assert.equal(notice.delay,4500);notice.fire();assert.equal(notice.closed,true);
 }
 for(const message of ['检测到未加密语音，已暂停连接','账号偏好同步：网络错误'])assert.equal(render(message).delay,undefined);
});
test('encryption warnings, failed sends and device changes never automatically disappear',()=>{
 for(const message of ['发送失败；可重试同一份密文。','对方身份已变化，私信停止。','已选择端到端加密，请启用设备身份并核对安全码。']){
  const notice=render(message);assert.equal(notice.delay,undefined);notice.fire();assert.equal(notice.closed,false);notice.tree.children[1].props.onClick();assert.equal(notice.closed,true);
 }
});
