export const defaultImageLimits={channel_download_kib:2048,channel_dimension:2048,channel_output_kib:192,channel_images:8,channel_cache:32,channel_requests:8,avatar_download_kib:128,avatar_upload_kib:64,avatar_dimension:512,avatar_source_mib:2};
export type ImageLimits=typeof defaultImageLimits;
export const imageLimitFields:{key:keyof ImageLimits;label:string;min:number;max:number}[]=[
  {key:'channel_download_kib',label:'频道图片原文件大小（KiB）',min:64,max:8192},
  {key:'channel_dimension',label:'频道图片解码最大边长（px）',min:128,max:4096},
  {key:'channel_output_kib',label:'频道图片压缩后大小（KiB）',min:16,max:192},
  {key:'channel_images',label:'每篇频道介绍最多加载图片数',min:1,max:32},
  {key:'channel_cache',label:'每次连接最多缓存频道图片数',min:1,max:128},
  {key:'channel_requests',label:'每个连接每10秒最多读取频道图片数',min:1,max:32},
  {key:'avatar_download_kib',label:'TS 成员头像原文件大小（KiB）',min:16,max:512},
  {key:'avatar_upload_kib',label:'保存与上传头像大小（KiB）',min:8,max:128},
  {key:'avatar_dimension',label:'头像解码最大边长（px）',min:96,max:1024},
  {key:'avatar_source_mib',label:'本地裁剪前头像原文件大小（MiB）',min:1,max:8},
];
export function readImageLimits(value:unknown):ImageLimits {
  const result={...defaultImageLimits};if(!value||typeof value!=='object')return result;
  for(const {key,min,max} of imageLimitFields){const number=(value as Record<string,unknown>)[key];if(typeof number==='number'&&Number.isInteger(number)&&number>=min&&number<=max)result[key]=number;}
  return result;
}
