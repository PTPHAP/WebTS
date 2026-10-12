import type {State} from './api';
import type {TsImageState} from './ChannelText';

export function serverIconUrl(info:State['serverInfo'],images:Record<string,TsImageState>):string {
  const native=info?.icon?`tsserver:icon:${info.icon}`:'';
  if(native&&(!images[native]?.error||!info?.buttonImage))return native;
  return info?.buttonImage?`tsserver:button:${info.buttonImage}`:'';
}
