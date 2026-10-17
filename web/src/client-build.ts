export function bundlePath(value:unknown):string|undefined {return typeof value==='string'&&/^\/assets\/index-[A-Za-z0-9_-]{1,100}\.js$/.test(value)?value:undefined;}
// Identify the loaded page, not the newest bundle offered by the server.
export function loadedBundle(doc:Pick<Document,'querySelectorAll'>,origin:string):string|undefined {
  for(const script of doc.querySelectorAll<HTMLScriptElement>('script[type="module"][src]')){
    try{const url=new URL(script.src,origin);if(url.origin===origin&&!url.search&&!url.hash){const bundle=bundlePath(url.pathname);if(bundle)return bundle;}}catch{/* Development and unknown entry points have no release identifier. */}
  }
}
