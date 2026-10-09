export function imageDimensions(bytes:Uint8Array):[number,number] {
  const view=new DataView(bytes.buffer,bytes.byteOffset,bytes.byteLength);
  if(bytes.length>=24&&bytes[0]===137&&bytes[1]===80&&bytes[2]===78&&bytes[3]===71)return [view.getUint32(16),view.getUint32(20)];
  if(bytes.length>=12&&bytes[0]===255&&bytes[1]===216){
    for(let offset=2;offset+4<=bytes.length;){
      if(bytes[offset]!==255)break;const marker=bytes[offset+1];if(marker===255){offset++;continue;}
      const length=view.getUint16(offset+2);if(length<2||offset+2+length>bytes.length)break;
      if([192,193,194,195,197,198,199,201,202,203,205,206,207].includes(marker)&&length>=7)return [view.getUint16(offset+7),view.getUint16(offset+5)];
      offset+=length+2;
    }
  }
  // WebP extended, lossless and lossy headers; never decode another format.
  if(bytes.length>=30&&String.fromCharCode(...bytes.slice(0,4))==='RIFF'&&String.fromCharCode(...bytes.slice(8,12))==='WEBP'){
    const kind=String.fromCharCode(...bytes.slice(12,16));
    if(kind==='VP8X')return [1+bytes[24]+(bytes[25]<<8)+(bytes[26]<<16),1+bytes[27]+(bytes[28]<<8)+(bytes[29]<<16)];
    if(kind==='VP8L'&&bytes[20]===47){const packed=view.getUint32(21,true);return [(packed&16383)+1,((packed>>>14)&16383)+1];}
    if(kind==='VP8 '&&bytes[23]===157&&bytes[24]===1&&bytes[25]===42)return [view.getUint16(26,true)&16383,view.getUint16(28,true)&16383];
  }
  throw new Error('图片格式或头部无效，请使用PNG、JPEG或WebP。');
}
export type Crop={x:number;y:number;width:number;height:number};
export type ImagePurpose='avatar'|'content';
export async function loadCropImage(file:File,purpose:ImagePurpose):Promise<ImageBitmap> {
  const limit=purpose==='avatar'?2:4;
  if(!['image/png','image/jpeg','image/webp'].includes(file.type)||file.size>limit*1024*1024)throw new Error(`请选择${limit}MiB以内的PNG、JPEG或WebP图片。`);
  const [width,height]=imageDimensions(new Uint8Array(await file.arrayBuffer()));
  if(!width||!height||width>4096||height>4096||width*height>8*1024*1024)throw new Error('图片尺寸过大，请先缩小图片。');
  const image=await createImageBitmap(file);
  if(!image.width||!image.height||image.width>4096||image.height>4096||image.width*image.height>8*1024*1024){image.close();throw new Error('图片尺寸过大，请先缩小图片。');}
  return image;
}
export function initialCrop(width:number,height:number,square:boolean):Crop {
  const side=Math.min(width,height);
  return square?{x:(width-side)/2,y:(height-side)/2,width:side,height:side}:{x:0,y:0,width,height};
}
export function adjustCrop(crop:Crop,width:number,height:number,dx:number,dy:number,handle:string,square:boolean):Crop {
  const clamp=(v:number,min:number,max:number)=>Math.max(min,Math.min(max,v));
  if(handle==='move')return {...crop,x:clamp(crop.x+dx,0,width-crop.width),y:clamp(crop.y+dy,0,height-crop.height)};
  const left=handle.includes('w'),top=handle.includes('n');
  const ax=left?crop.x+crop.width:crop.x,ay=top?crop.y+crop.height:crop.y;
  const maxW=left?ax:width-ax,maxH=top?ay:height-ay,min=Math.min(16,maxW,maxH);
  let w=clamp(crop.width+(left?-dx:dx),min,maxW),h=clamp(crop.height+(top?-dy:dy),min,maxH);
  if(square)w=h=clamp(crop.width+(Math.abs(dx)>=Math.abs(dy)?(left?-dx:dx):(top?-dy:dy)),min,Math.min(maxW,maxH));
  return {x:left?ax-w:ax,y:top?ay-h:ay,width:w,height:h};
}
export function croppedImage(image:ImageBitmap,crop:Crop,purpose:ImagePurpose):string {
  if(!Object.values(crop).every(Number.isFinite)||crop.x<0||crop.y<0||crop.width<1||crop.height<1||crop.x+crop.width>image.width+.001||crop.y+crop.height>image.height+.001)throw new Error('裁剪范围无效，请重置后再试。');
  if(purpose==='avatar'&&Math.abs(crop.width-crop.height)>.001)throw new Error('头像需要正方形裁剪。');
  const canvas=document.createElement('canvas');
  for(const size of purpose==='avatar'?[256,128,96]:[1280,960,640]){
    const scale=purpose==='avatar'?size/crop.width:Math.min(1,size/crop.width,960/crop.height);
    canvas.width=Math.max(1,Math.round(crop.width*scale));canvas.height=Math.max(1,Math.round(crop.height*scale));
    const context=canvas.getContext('2d');if(!context)throw new Error('浏览器无法处理图片');
    if(purpose==='content'){context.fillStyle='#151b2c';context.fillRect(0,0,canvas.width,canvas.height);}
    context.drawImage(image,crop.x,crop.y,crop.width,crop.height,0,0,canvas.width,canvas.height);
    const result=canvas.toDataURL(purpose==='avatar'?'image/png':'image/jpeg',.75);
    const data=purpose==='avatar'?result.split(',')[1]:result;
    if(data.length<=(purpose==='avatar'?Math.ceil(65536/3)*4:170000))return data;
  }
  throw new Error('图片压缩后仍过大，请缩小裁剪范围或更换图片。');
}
export async function avatarImage(file:File):Promise<string> {
  const image=await loadCropImage(file,'avatar');
  try{return croppedImage(image,initialCrop(image.width,image.height,true),'avatar');}finally{image.close();}
}
