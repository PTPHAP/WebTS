import {useEffect,useRef,useState} from 'react';
import type {KeyboardEvent,PointerEvent} from 'react';
import {createPortal} from 'react-dom';
import {adjustCrop,croppedImage,initialCrop,loadCropImage,zoomCrop} from './avatar';
import type {Crop,ImagePurpose} from './avatar';

export function ImageCropper({file,purpose,onComplete,onCancel}:{file:File;purpose:ImagePurpose;onComplete:(image:string)=>void;onCancel:()=>void}){
  const dialog=useRef<HTMLDialogElement>(null),canvas=useRef<HTMLCanvasElement>(null),image=useRef<ImageBitmap|null>(null);
  const[dimensions,setDimensions]=useState<[number,number]>([0,0]),[crop,setCrop]=useState<Crop|null>(null),[zoom,setZoom]=useState(1),[message,setMessage]=useState('正在读取图片…');
  const drag=useRef<{id:number;x:number;y:number;crop:Crop;handle:string;scale:[number,number]}|null>(null);
  const square=purpose==='avatar';
  useEffect(()=>{
    let cancelled=false;dialog.current?.showModal();
    loadCropImage(file,purpose).then(bitmap=>{if(cancelled){bitmap.close();return;}image.current=bitmap;setDimensions([bitmap.width,bitmap.height]);setCrop(initialCrop(bitmap.width,bitmap.height,square));setMessage('');}).catch(e=>{if(!cancelled)setMessage(e instanceof Error?e.message:'读取图片失败');});
    return()=>{cancelled=true;image.current?.close();image.current=null;dialog.current?.close();};
  },[file,purpose,square]);
  const[width,height]=dimensions,scale=Math.min(600/(width||1),360/(height||1),1);
  const previewWidth=Math.max(1,Math.round(width*scale)),previewHeight=Math.max(1,Math.round(height*scale));
  useEffect(()=>{if(!image.current||!canvas.current)return;const context=canvas.current.getContext('2d');context?.drawImage(image.current,0,0,canvas.current.width,canvas.current.height);},[width,height]);
  function start(e:PointerEvent<HTMLDivElement>){
    if(!crop||e.button!==0)return;
    const handle=(e.target as HTMLElement).dataset.handle||'move';
    e.preventDefault();e.currentTarget.setPointerCapture(e.pointerId);
    const bounds=e.currentTarget.getBoundingClientRect();
    drag.current={id:e.pointerId,x:e.clientX,y:e.clientY,crop,handle,scale:[width/bounds.width,height/bounds.height]};
  }
  function move(e:PointerEvent<HTMLDivElement>){const d=drag.current;if(!d||d.id!==e.pointerId)return;setCrop(adjustCrop(d.crop,width,height,(e.clientX-d.x)*d.scale[0],(e.clientY-d.y)*d.scale[1],d.handle,square));}
  function keys(e:KeyboardEvent<HTMLElement>,handle:string){
    if(!crop||!['ArrowLeft','ArrowRight','ArrowUp','ArrowDown'].includes(e.key))return;e.preventDefault();e.stopPropagation();
    const step=e.shiftKey?10:1;
    setCrop(adjustCrop(crop,width,height,e.key==='ArrowLeft'?-step:e.key==='ArrowRight'?step:0,e.key==='ArrowUp'?-step:e.key==='ArrowDown'?step:0,handle,square));
  }
  function changeZoom(value:number){if(!crop)return;setZoom(value);setCrop(zoomCrop(crop,width,height,zoom/value));}
  function confirm(){if(!crop||!image.current)return;try{onComplete(croppedImage(image.current,crop,purpose));}catch(e){setMessage(e instanceof Error?e.message:'裁剪失败');}}
  return createPortal(<dialog ref={dialog} className="image-cropper" aria-labelledby="crop-title" onCancel={e=>{e.preventDefault();e.stopPropagation();onCancel();}}>
    <div className="dialog-head"><h2 id="crop-title">{square?'裁剪头像':'裁剪图片'}</h2><button type="button" className="icon-button" aria-label="取消裁剪" onClick={onCancel}>×</button></div>
    <p className="muted">拖动图片移动选区，拖动四角调整范围。{square?'头像保持正方形。':'图片可自由调整宽高。'}键盘方向键微调，Shift 加速。</p>
    {crop&&<><div className="crop-stage" style={{width:previewWidth,aspectRatio:`${previewWidth}/${previewHeight}`}} onPointerDown={start} onPointerMove={move} onPointerUp={()=>{drag.current=null;}} onPointerCancel={()=>{drag.current=null;}} onLostPointerCapture={()=>{drag.current=null;}}>
      <canvas ref={canvas} width={previewWidth} height={previewHeight} aria-label="原图与裁剪范围"/>
      <div className="crop-selection" role="group" tabIndex={0} aria-label="移动裁剪选区" onKeyDown={e=>keys(e,'move')} style={{left:`${crop.x/width*100}%`,top:`${crop.y/height*100}%`,width:`${crop.width/width*100}%`,height:`${crop.height/height*100}%`}}>
        <span className="crop-grid"/>{(['nw','ne','sw','se'] as const).map((handle,i)=><button key={handle} type="button" data-handle={handle} className={`crop-handle ${handle}`} aria-label={`${['左上','右上','左下','右下'][i]}裁剪角`} onKeyDown={e=>keys(e,handle)}/>)}
      </div>
    </div><label>缩放选区<input type="range" min="1" max="8" step=".05" value={zoom} onChange={e=>changeZoom(Number(e.target.value))}/></label><small className="muted">裁剪范围 {Math.round(crop.width)} × {Math.round(crop.height)} px · 仅使用选区内的内容</small></>}
    {message&&<p role="status" className="auth-help">{message}</p>}
    <div className="crop-actions"><button type="button" className="text-button" disabled={!crop} onClick={()=>{setCrop(initialCrop(width,height,square));setZoom(1);setMessage('');}}>重置</button><button type="button" className="secondary" onClick={onCancel}>取消</button><button type="button" className="primary" disabled={!crop} onClick={confirm}>使用裁剪图片</button></div>
  </dialog>,document.body);
}
