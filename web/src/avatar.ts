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
export async function avatarImage(file:File):Promise<string> {
  if(!['image/png','image/jpeg','image/webp'].includes(file.type)||file.size>2*1024*1024)throw new Error('请选择2MiB以内的PNG、JPEG或WebP图片。');
  const [width,height]=imageDimensions(new Uint8Array(await file.arrayBuffer()));
  if(!width||!height||width>4096||height>4096||width*height>8*1024*1024)throw new Error('图片尺寸过大，请先缩小图片。');
  const image=await createImageBitmap(file);
  try {
    if(image.width>8192||image.height>8192)throw new Error('图片尺寸过大，请先缩小图片。');
    const canvas=document.createElement('canvas');
    for(const size of [256,128,96]) {
      canvas.width=size;canvas.height=size;
      const context=canvas.getContext('2d');if(!context)throw new Error('浏览器不能处理头像');
      const side=Math.min(image.width,image.height);context.drawImage(image,(image.width-side)/2,(image.height-side)/2,side,side,0,0,size,size);
      const data=canvas.toDataURL('image/png').split(',')[1];
      if(data.length<=Math.ceil(65536/3)*4)return data;
    }
    throw new Error('图片压缩后仍过大，请更换图片。');
  } finally {image.close();}
}
