import {memo,useId} from 'react';
import {readSeason} from './seasons';

// Authored SVG only. Never render uploaded markup, external URLs or scripts here.
export const SeasonArt=memo(function SeasonArt({value,compact=false,className=''}:{value:unknown;compact?:boolean;className?:string}){
  const id=useId().replace(/[^a-zA-Z0-9_-]/g,''),season=readSeason(value);
  if(season==='classic')return null;
  const warm=`${id}-warm`,cool=`${id}-cool`;
  return <svg className={`season-art ${compact?'season-art-compact':''} ${className}`} data-festival={season} viewBox={compact?'146 18 206 230':'0 0 480 280'} fill="none" aria-hidden="true" focusable="false">
    <defs><linearGradient id={warm} x1="160" y1="40" x2="300" y2="250" gradientUnits="userSpaceOnUse"><stop stopColor="#ffe8ae"/><stop offset="1" stopColor="#efad55"/></linearGradient><linearGradient id={cool} x1="170" y1="45" x2="300" y2="240" gradientUnits="userSpaceOnUse"><stop stopColor="#b6d3ff"/><stop offset="1" stopColor="#7d8ee8"/></linearGradient></defs>
    <ellipse cx="242" cy="252" rx="153" ry="12" fill="currentColor" opacity=".08"/>
    <g className="season-art-halo" stroke="currentColor" opacity=".12"><circle cx="244" cy="133" r="106"/><circle cx="244" cy="133" r="119" strokeDasharray="2 12"/></g>
    {season==='spring'&&<>
      <g stroke="#d9905b" strokeWidth="3" strokeLinecap="round"><path d="M244 23V58M109 83V121M367 48V108"/><path d="M71 182q29-34 60-19M85 171l-7-28M102 163l19-23"/></g>
      <g className="season-art-main"><rect x="209" y="49" width="68" height="15" rx="6" fill={`url(#${warm})`}/><path d="M201 65q-36 16-36 73t36 72h84q37-16 37-72t-37-73Z" fill="#c84147"/><ellipse cx="243" cy="138" rx="55" ry="75" fill="#e65c54"/><ellipse cx="243" cy="138" rx="27" ry="75" stroke="#ffbc71" strokeWidth="2" opacity=".65"/><path d="M171 110h145M168 166h151" stroke="#ffcd8e" opacity=".5"/><path d="m244 100 37 38-37 38-37-38Z" fill={`url(#${warm})`}/><text x="244" y="146" textAnchor="middle" fontSize="24" fontWeight="700" fill="#b6383b">福</text><rect x="209" y="207" width="68" height="13" rx="5" fill={`url(#${warm})`}/><path d="M244 221v25m-7-15v17m14-17v17" stroke="#e6ab57" strokeWidth="4" strokeLinecap="round"/></g>
      <g transform="translate(86 110)"><g className="season-art-side-lantern"><rect width="46" height="61" rx="20" fill="#df6a56"/><path d="M9 5h28M8 54h30M23 62v18" stroke="#f5cb85" strokeWidth="4"/></g></g>
      <g transform="translate(344 115) rotate(-12)"><path d="M9 0h32M25 0v102" stroke="#d99f60" strokeWidth="3"/>{[7,34,61].map(y=><g key={y}><rect x="0" y={y} width="19" height="26" rx="4" fill="#cd424b"/><rect x="30" y={y+10} width="19" height="26" rx="4" fill="#ec6c56"/><path d={`M2 ${y+5}h15M2 ${y+21}h15M32 ${y+15}h15M32 ${y+31}h15`} stroke="#f4c379" strokeWidth="2"/></g>)}</g>
      {[{x:80,y:146},{x:121,y:140},{x:91,y:183}].map(({x,y})=><g key={x} transform={`translate(${x} ${y})`} fill="#f8aca5"><g className="season-art-blossom"><circle cx="0" cy="-5" r="6"/><circle cx="5" cy="0" r="6"/><circle cx="0" cy="5" r="6"/><circle cx="-5" cy="0" r="6"/><circle r="3" fill="#ffe1a1"/></g></g>)}
    </>}
    {season==='midautumn'&&<>
      <circle cx="244" cy="125" r="88" fill={`url(#${warm})`}/><g fill="#e5a95c" opacity=".28"><circle cx="201" cy="100" r="16"/><circle cx="279" cy="75" r="9"/><circle cx="292" cy="130" r="13"/></g>
      <g className="season-art-clouds" stroke="#b0c8e9" strokeWidth="2" fill="var(--panel)"><path d="M100 151h75q19 0 19-12t-17-12h-10q-1-24-22-24t-24 21h-17q-18 0-18 14t14 13Z"/><path d="M319 104h64q17 0 17-12t-15-12h-9q-3-21-21-21t-21 20h-13q-16 0-16 12t14 13Z"/></g>
      <g className="season-art-main"><ellipse cx="252" cy="216" rx="51" ry="31" fill="#fff1d7"/><ellipse cx="261" cy="174" rx="32" ry="30" fill="#fff6e7"/><ellipse cx="246" cy="128" rx="10" ry="35" transform="rotate(-13 246 128)" fill="#fff6e7"/><ellipse cx="273" cy="128" rx="10" ry="35" transform="rotate(9 273 128)" fill="#fff6e7"/><path d="m243 110 4 28m28-30-2 28" stroke="#edbcab" strokeWidth="5" strokeLinecap="round"/><circle cx="276" cy="172" r="3" fill="#79576a"/><circle cx="280" cy="183" r="4" fill="#f0b5a1"/><circle cx="204" cy="212" r="12" fill="#fff6e7"/><path d="M252 192q-13 14-7 25" stroke="#e8cdae" strokeWidth="3" strokeLinecap="round"/></g>
      <g transform="translate(98 207)"><ellipse cx="32" cy="23" rx="35" ry="13" fill="#b87943"/><rect y="3" width="64" height="21" rx="10" fill="#d6a05a"/><ellipse cx="32" cy="5" rx="32" ry="14" fill="#f4ce8c"/><path d="M15 5q17-19 34 0-17 19-34 0Zm17-9v18" stroke="#c18b4c" strokeWidth="2"/></g>
      <g stroke="#c79952" strokeWidth="2" strokeLinecap="round"><path d="m355 235 16-62m-7 29 25-13m-20-5-15-16"/>{[0,1,2].map(i=><g key={i} transform={`translate(${361+i*11} ${180+i*12})`}><path d="M-5 0H5M0-5V5" stroke="#ebbd63" strokeWidth="4"/></g>)}</g>
    </>}
    {season==='winter'&&<>
      <g className="season-art-snow" stroke="#a8d7de" strokeWidth="2" strokeLinecap="round">{[{x:108,y:84},{x:368,y:101},{x:330,y:43}].map(({x,y})=><path key={x} d={`M${x-11} ${y}h22m-11-11v22m-8-19 16 16m-16 0 16-16`}/>)}</g>
      <g className="season-art-main"><rect x="234" y="197" width="23" height="42" rx="5" fill="#b38565"/><path d="m245 49-52 71h28l-50 62h35l-47 48h172l-47-48h35l-50-62h28Z" fill="#267e68"/><path d="m245 49-52 71h28l-50 62h35l-47 48h52l35-181Z" fill="#3b9c7b"/><path d="M211 119q35 13 67-1m-84 61q51 17 101 0" stroke="#edd191" strokeWidth="4" strokeLinecap="round"/><path d="m245 24 7 14 16 2-11 11 3 16-15-7-14 7 3-16-12-11 16-2Z" fill={`url(#${warm})`}/><g className="season-art-lights" fill="#efad86"><circle cx="229" cy="101" r="6"/><circle cx="266" cy="150" r="7"/><circle cx="220" cy="203" r="7"/></g><g className="season-art-lights season-art-lights-late" fill="#e7d2a0"><circle cx="211" cy="152" r="5"/><circle cx="284" cy="203" r="5"/></g></g>
      <g transform="translate(93 197) rotate(-8)"><rect y="9" width="63" height="42" rx="6" fill="#c75b67"/><rect y="4" width="63" height="13" rx="4" fill="#e48388"/><path d="M30 5v46M30 5q-26 0-19-12T30 5q25-1 18-12T30 5" stroke="#f5d7a4" strokeWidth="6"/></g>
      <g transform="translate(335 207)"><rect y="5" width="44" height="35" rx="5" fill="#9bbcd2"/><path d="M22 5v34M1 15h42M22 5Q2-8 8-13T22 5Q44-8 36-13T22 5" stroke="#f3d4a0" strokeWidth="5"/></g>
      <path d="M352 144v47m-8-43q8-9 16 0v20" stroke="#e79e92" strokeWidth="7" strokeLinecap="round"/>
    </>}
    {season==='halloween'&&<>
      <path d="M355 54q-35 48 16 65-63 4-58-38 1-25 42-27Z" fill={`url(#${warm})`}/>
      <g className="season-art-bats" fill="#9883c5"><path d="m89 101 17-12 7 17 7-17 18 12-11 5-5 14-9-7-9 7-5-14Z"/><path d="m347 152 17-12 7 17 7-17 18 12-11 5-5 14-9-7-9 7-5-14Z"/></g>
      <g className="season-art-main"><path d="M240 122q-9-25 16-31" stroke="#6e8a68" strokeWidth="12" strokeLinecap="round"/><path d="M173 147q5-38 45-31 25-11 52 0 43-4 48 31 23 62-30 86-42 20-85 0-52-18-30-86Z" fill="#e2944d"/><ellipse cx="244" cy="176" rx="46" ry="65" fill="#f3ac58"/><path d="M218 117q-36 54-8 113m61-113q35 54 7 113" stroke="#d47840" strokeWidth="3"/><g fill="#5b4769"><path d="m219 150-17 23h28Zm51 0-10 23h29Zm-26 26-8 12h16Zm-38 23 14 5 6-8 13 10 12-10 9 9 19-6q-8 27-36 24t-37-24Z"/></g><path d="m193 104 40-69 40 61 33 15q-68 21-120 6Z" fill="#6e559a"/><path d="m207 88 61-9 10 16-82 15Z" fill="#c5a164"/><rect x="240" y="83" width="16" height="17" rx="3" stroke="#ffe0a2" strokeWidth="3"/></g>
      <g transform="translate(102 208)"><rect width="23" height="34" rx="5" fill="#e6d7bb"/><path className="season-art-spark" d="M11-26q-16 19 0 23 14-4 0-23Z" fill="#f2bb67"/><path d="M8 1v12m9-12v6" stroke="#f8edd2" strokeWidth="5" strokeLinecap="round"/></g>
    </>}
    {season==='newyear'&&<>
      <g strokeWidth="3" strokeLinecap="round" className="season-art-fireworks"><path d="M109 64V44m0 68V92M85 78H65m88 0h-20M91 60 78 47m62 62-13-13m0-36 13-13m-49 49-13 13" stroke="#e9b988"/><path d="M367 132v-15m0 56v-15m-14-13h-15m58 0h-14m-29-14-10-10m40 40 10 10m-10-40 10-10m-40 40-10 10" stroke="#a8baf7"/></g>
      <g fill="#849bd6" opacity=".45"><path d="M132 238v-27h20v-24h23v51Zm193 0v-40h19v-22h18v62Z"/><path d="M106 244v-14h276v14Z"/></g>
      <g className="season-art-main"><circle cx="245" cy="142" r="87" fill={`url(#${cool})`}/><circle cx="245" cy="142" r="76" fill="var(--panel)"/><circle cx="245" cy="142" r="66" stroke="#b6c5ed" strokeWidth="2" strokeDasharray="2 10"/><path d="M245 87v10m55 45h-10m-45 55v-10m-55-45h10" stroke="#a6b9e9" strokeWidth="4" strokeLinecap="round"/><path d="M245 142V103m0 39 28-30" stroke="#e0b786" strokeWidth="6" strokeLinecap="round"/><path className="season-art-second-hand" d="M245 142V90" stroke="#d48d90" strokeWidth="2"/><circle cx="245" cy="142" r="6" fill="#e0b786"/><path d="M210 231h70l8 13h-86Z" fill="#9baee2"/><path d="m245 20 5 10 11 1-8 8 2 11-10-5-10 5 2-11-8-8 11-1Z" fill={`url(#${warm})`}/></g>
      <g strokeWidth="4" strokeLinecap="round"><path d="m143 149-5 10m197-91 7 6m-210 94 7 7" stroke="#dbaf94"/><path d="m330 96 4-11m-199 36-10 1m203 78 8 6" stroke="#9baeed"/></g>
    </>}
    <g className="season-art-spark" fill="currentColor" opacity=".7"><path d="m153 45 3 7 7 3-7 3-3 7-3-7-7-3 7-3Z"/><path d="m331 213 2 5 5 2-5 2-2 5-2-5-5-2 5-2Z"/><circle cx="399" cy="196" r="3"/><circle cx="79" cy="216" r="2"/></g>
  </svg>;
});
