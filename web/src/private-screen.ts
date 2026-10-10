import {useEffect,useState} from 'react';
// Browsers cannot prohibit OS screenshots or capture by other applications.
// This default mask only reduces accidental disclosure when leaving the page.
export function usePrivateScreen(){const[hidden,setHidden]=useState(()=>document.hidden||!document.hasFocus());useEffect(()=>{const update=()=>setHidden(document.hidden||!document.hasFocus());window.addEventListener('blur',update);window.addEventListener('focus',update);document.addEventListener('visibilitychange',update);update();return()=>{window.removeEventListener('blur',update);window.removeEventListener('focus',update);document.removeEventListener('visibilitychange',update);};},[]);return hidden;}
