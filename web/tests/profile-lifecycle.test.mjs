import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import ts from 'typescript';

const moduleURL=code=>`data:text/javascript;base64,${Buffer.from(code).toString('base64')}`;
const hooks=moduleURL(`export const useRef=value=>({current:value});export const useState=value=>[value,v=>globalThis.profileFixture.updates.push(v)];export const useEffect=effect=>{globalThis.profileFixture.cleanup=effect();};`);
const jsx=moduleURL(`export const jsx=(type,props)=>({type,props});export const jsxs=jsx;`);
const api=moduleURL(`export const api=(...args)=>globalThis.profileFixture.api(...args);`);
const avatar=moduleURL(`export const avatarImage=()=>Promise.resolve('');`);
const code=ts.transpileModule(await readFile(new URL('../src/ProfileSettings.tsx',import.meta.url),'utf8'),{compilerOptions:{module:ts.ModuleKind.ESNext,target:ts.ScriptTarget.ES2022,jsx:ts.JsxEmit.ReactJSX}}).outputText.replace('react/jsx-runtime',jsx).replace("from 'react'",`from '${hooks}'`).replace("from './api'",`from '${api}'`).replace("from './avatar'",`from '${avatar}'`);
const {ProfileSettings,emptyProfile}=await import(moduleURL(code));
test('a delayed saved profile cannot update a new account after its editor unmounts',async()=>{
  let resolve;const saved=[];
  globalThis.profileFixture={updates:[],cleanup:null,api:()=>new Promise(r=>{resolve=r;})};
  const form=ProfileSettings({profile:emptyProfile,email:'old-account@example.com',onSaved:p=>saved.push(p)});
  const pending=form.props.onSubmit({preventDefault(){}});
  profileFixture.cleanup();profileFixture.updates.length=0;
  resolve({...emptyProfile,display_name:'Old account private profile'});await pending;
  assert.deepEqual(saved,[]);assert.deepEqual(profileFixture.updates,[]);
});
test('a mounted profile editor applies its own successful save',async()=>{
  const saved=[],data={...emptyProfile,display_name:'Current account'};
  globalThis.profileFixture={updates:[],cleanup:null,api:async()=>data};
  const form=ProfileSettings({profile:emptyProfile,email:'current-account@example.com',onSaved:p=>saved.push(p)});
  await form.props.onSubmit({preventDefault(){}});assert.deepEqual(saved,[data]);profileFixture.cleanup();
});
