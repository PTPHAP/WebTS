import {useEffect,useRef} from 'react';

const temporary=[
  '加密私信已送达','阅后即焚密文已发送，对方成功解密并确认已读后开始倒计时。',
  '已加密保存至对象存储 · 对方上线可读取','待发密文已送达','好友码已复制',
  '请求已发送，对方接受后可核对安全码。','好友身份已固定在此设备。',
  '已选择普通聊天，可直接发送，对方离线也会加密保存至对象存储。',
  '此设备已自动解锁。密钥留在本浏览器；可在“设备与备份”中选择备份。','私信已解锁。',
  '已下载受独立口令保护的身份备份。口令不发送给网站。','待发密文已放弃，棘轮状态已保留。',
];
export function ChatNotice({message,dismiss}:{message:string;dismiss:()=>void}){
  const close=useRef(dismiss);close.current=dismiss;
  useEffect(()=>{if(!temporary.includes(message))return;const timer=setTimeout(()=>close.current(),4500);return()=>clearTimeout(timer);},[message]);
  if(!message)return null;
  return <aside className="chat-notice" role="status"><span>{message}</span><button className="icon-button" aria-label="关闭聊天提示" onClick={dismiss}>×</button></aside>;
}
