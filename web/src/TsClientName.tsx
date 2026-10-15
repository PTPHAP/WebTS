// Display only the gateway-provided connection flag; this never grants permissions.
export function TsClientName({name,webts}:{name:string;webts?:boolean}) {
  return <span className="ts-client-name"><span>{name}</span>{webts===true&&<small className="webts-client-tag" title="WebTS 网页连接标签（非身份认证）">WebTS</small>}</span>;
}
