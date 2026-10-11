import test from 'node:test';
import assert from 'node:assert/strict';
import {readFile} from 'node:fs/promises';
import postcss from 'postcss';

const legacy=await readFile(new URL('../src/style.css',import.meta.url),'utf8');
test('compact workspace styles follow the chosen mode rather than width or pointer changes',()=>{
  const root=postcss.parse(legacy);let workspaceRules=0;
  root.walkRules(rule=>{
    if(!rule.selector.includes('.workspace-pane')||!rule.nodes.some(d=>d.prop==='display'&&d.value==='none'))return;
    workspaceRules++;
    assert.ok(rule.selector.includes('[data-layout=mobile]'),rule.selector);
    for(let parent=rule.parent;parent;parent=parent.parent)assert.ok(parent.type!=='atrule'||parent.name!=='media'||!parent.params.includes('max-width'),parent.params);
  });
  assert.ok(workspaceRules>0);
});
test('shrinking a desktop window does not push the entire header and footer off screen',()=>{
  const root=postcss.parse(legacy);
  root.walkRules(rule=>{
    if(rule.selector.startsWith('.app[data-layout=desktop]')&&!rule.selector.includes('>'))assert.equal(rule.nodes.some(d=>d.prop==='min-width'&&d.value!=='0'),false,rule.toString());
  });
});
test('compact navigation keeps friend and account entries available',()=>{
  postcss.parse(legacy).walkRules(rule=>{
    if(rule.selector.includes('.mobile-navigation button'))assert.equal(rule.nodes.some(d=>d.prop==='display'&&d.value==='none'),false,rule.selector);
  });
});
test('visual layer uses finite motion, respects reduced motion and loads no external assets',async()=>{
  const design=await readFile(new URL('../src/interface.css',import.meta.url),'utf8');
  assert.doesNotMatch(design,/url\s*\(|@import|infinite|transition\s*:\s*all/);
  assert.match(design,/@media\(prefers-reduced-motion:reduce\)/);
  assert.match(design,/animation:none!important/);
  assert.match(design,/scroll-behavior:auto!important/);
});
