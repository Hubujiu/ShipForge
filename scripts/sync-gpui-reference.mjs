import { createHash } from 'node:crypto';
import { readFileSync, readdirSync, writeFileSync } from 'node:fs';
import { fileURLToPath } from 'node:url';
import { join } from 'node:path';

const root = fileURLToPath(new URL('../', import.meta.url));
const native = join(root, 'crates/shipforge-desktop');
const assets = join(native, 'assets');
const demo = JSON.parse(readFileSync(join(assets, 'demo.json'), 'utf8'));
const icons = readdirSync(join(assets, 'icons')).filter(n => n.endsWith('.svg')).sort();
const registration = readFileSync(join(native, 'src/assets.rs'), 'utf8');
const names = [...registration.matchAll(/"(icons\/[a-z0-9-]+\.svg)"\s*=>/g)].map(m => m[1].slice(6)).sort();
if (JSON.stringify(names) !== JSON.stringify(icons)) throw Error('The native asset registry must exactly match the icon directory.');
const esc = x => String(x).replaceAll('&', '&amp;').replaceAll('<', '&lt;').replaceAll('>', '&gt;').replaceAll('"', '&quot;');
const status = {todo:'待办', in_progress:'进行中', done:'已完成'};
const priorities = {low:'低', medium:'中', high:'高'};
const rows = demo.filter(r => r.kind === 'task');
const hash = createHash('sha256');
for (const n of icons) hash.update(n).update(readFileSync(join(assets, 'icons', n), 'utf8').replaceAll('\r\n', '\n'));
const boards = Object.entries(status).map(([key,label]) => '<section><h2>' + label + '</h2>' +
  rows.filter(r => r.fields.status === key).map(r => '<article><strong>' + esc(r.title) + '</strong><p>' + esc(r.fields.description || '') + '</p><small>' + esc(priorities[r.fields.priority] || '') + '优先级</small></article>').join('') + '</section>').join('');
const svgList = icons.map(n => '<li><img alt="" src="../../crates/shipforge-desktop/assets/icons/' + n + '"><span>' + esc(n.slice(0,-4)) + '</span></li>').join('');
const html = '<!doctype html>\n<html lang="zh-CN"><meta charset="utf-8"><meta name="viewport" content="width=device-width, initial-scale=1"><title>ShipForge 工作台参考</title>\n' +
  '<style>body{margin:0;padding:32px;font:14px/1.5 system-ui,sans-serif;background:#fcfcfd;color:#23242a}h1{font-size:24px}h2{font-size:15px}main{max-width:1280px;margin:auto}.board{display:grid;grid-template-columns:repeat(auto-fit,minmax(190px,1fr));gap:16px}section{padding:12px;border-radius:10px;background:#f5f5f8}article{padding:12px;margin:.5rem 0;background:white;border:1px solid #e5e6ec;border-radius:8px}p,small{color:#686b77}ul{padding:0;display:grid;grid-template-columns:repeat(auto-fit,minmax(160px,1fr));gap:8px}li{display:flex;align-items:center;gap:8px}img{width:17px;height:17px}</style>\n' +
  '<main><h1>空间任务</h1><div class="board">' + boards + '</div><h2>工作台图标</h2><ul>' + svgList + '</ul></main>\n' +
  '<!-- Native Lucide assets SHA-256 ' + hash.digest('hex') + ' -->\n</html>\n';
const output = join(root, 'prototypes/workbench/index.html');
if (process.argv.includes('--check')) {
  if (readFileSync(output, 'utf8').replaceAll('\r\n', '\n') !== html) throw Error('Run node scripts/sync-gpui-reference.mjs to update the HTML reference.');
} else {
  writeFileSync(output, html);
}
console.log('Verified ' + icons.length + ' Lucide icons and ' + rows.length + ' native demo tasks.');
