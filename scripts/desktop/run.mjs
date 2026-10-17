// Real macOS WKWebView -> React controls -> Tauri IPC -> SQLCipher.
// No installed-library mode. Every run owns its temporary fixture and state.
import assert from 'node:assert/strict';
import { createHash, randomUUID } from 'node:crypto';
import { expanded } from './expanded.mjs';
import { spawn, spawnSync } from 'node:child_process';
import { createConnection, createServer } from 'node:net';
import { mkdtempSync, mkdirSync, openSync, closeSync, writeFileSync, readFileSync, unlinkSync } from 'node:fs';
import { tmpdir, hostname, cpus } from 'node:os';
import path from 'node:path';
import { fileURLToPath } from 'node:url';
import { once } from 'node:events';

const repo = fileURLToPath(new URL('../../', import.meta.url));
const reuse = process.argv.includes('--no-build');
const expandedOnly=process.argv.includes('--expanded-only');
if (process.platform !== 'darwin') throw new Error('Desktop tests require macOS and a logged-in GUI session.');
const macOS=spawnSync('/usr/bin/sw_vers',['-productVersion'],{encoding:'utf8'}).stdout?.trim();
assert.ok(Number(macOS?.split('.')[0])>=14,'Desktop tests require macOS 14+ for isolated WebKit stores');
if (process.argv.slice(2).some(a => !['--no-build','--expanded-only'].includes(a))) throw new Error('Usage: pnpm desktop:test [--no-build] [--expanded-only]');
const root = mkdtempSync(path.join(tmpdir(), 'rbx-desktop-'));
const library = path.join(root, 'library');
const destination = path.join(root, 'Desktop USB');
mkdirSync(destination);
const native = path.join(repo,'target/debug/desktop-native-input');
const verifier = path.join(repo,'target/debug/examples/desktop_verify');
const out = path.join(repo, 'desktop-test-results', path.basename(root));
mkdirSync(out, { recursive: true });
const results = [];
let child;
let app;
let runtime;
let launches=0;
let failure;
let binarySha;
let ownsLock = false;
const lockFile=path.join(tmpdir(),`rbx-desktop-${process.getuid()}.lock`);
const env = { ...process.env, RB_LITE_TEST: '1', RBXPORT_TEST: '1', RBXPORT_STATE_DIR: path.join(root, 'state'),
  RBXPORT_LOG_DIR: path.join(out, 'logs'), RBXPORT_OPTIONS: path.join(library, 'options.json'),
  MACOSX_DEPLOYMENT_TARGET:'11.0', RBXPORT_TEST_FIXTURE: library, RBXPORT_TEST_DATA_STORE: randomUUID(), RB_LITE_FAKE_VOLUMES: destination };
// Never inherit an unsafe-write override or a snapshot request.
delete env.RBX_DISABLE_READ_ONLY;
delete env.RBXPORT_DESKTOP_SNAPSHOT;
function command(binary, args, extra = {}) {
  const r = spawnSync(binary, args, { cwd: repo, env, encoding: 'utf8', ...extra });
  if (r.error || r.status !== 0) throw new Error(`${binary} ${args.join(' ')} failed: ${r.error ?? r.stderr ?? r.status}`);
  return r.stdout;
}
const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
async function until(check, description, timeout = 15000) {
  const end = Date.now() + timeout;
  let last;
  do {
    try { if (await check()) return; } catch (error) { last = error; }
    await sleep(100);
  } while (Date.now() < end);
  throw new Error(`Timed out waiting for ${description}${last ? `: ${last.message}` : ''}`);
}
class Desktop {
  constructor(socket) {
    this.socket = socket;
    this.id = 0;
    this.pending = new Map();
    let buffer = '';
    socket.setEncoding('utf8');
    socket.on('data', chunk => {
      buffer += chunk;
      while (buffer.includes('\n')) {
        const cut = buffer.indexOf('\n');
        const reply = JSON.parse(buffer.slice(0, cut));
        buffer = buffer.slice(cut + 1);
        const p = this.pending.get(reply.id);
        if (p) { clearTimeout(p.timer); this.pending.delete(reply.id); reply.ok ? p.resolve(reply.value) : p.reject(new Error(reply.error)); }
      }
    });
    const fail = error => {
      for (const p of this.pending.values()) { clearTimeout(p.timer); p.reject(error); }
      this.pending.clear();
    };
    socket.on('error', fail);
    socket.on('close', () => fail(new Error('Desktop app disconnected')));
  }
  request(body, timeout = 5000) {
    const id = ++this.id;
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        this.pending.delete(id);
        reject(new Error(`Desktop did not respond within ${timeout}ms: ${JSON.stringify(body).slice(0, 200)}`));
        this.socket.destroy();
      }, timeout);
      this.pending.set(id, { resolve, reject, timer });
      this.socket.write(JSON.stringify({ id, ...body }) + '\n');
    });
  }
  eval(js, window = 'main') { return this.request({ js, window }); }
  async click(selector, { text, event = 'click', window = 'main' } = {}) {
    await until(()=>this.eval(`[...document.querySelectorAll(${JSON.stringify(selector)})].some(e=>${text===undefined?'true':`e.textContent.includes(${JSON.stringify(text)})`})`,window),`control ${selector}${text?` (${text})`:''}`);
    await this.eval(`(() => {
      const el = [...document.querySelectorAll(${JSON.stringify(selector)})].find(e => ${text === undefined ? 'true' : `e.textContent.includes(${JSON.stringify(text)})`});
      if (!el) throw new Error('Missing control: ' + ${JSON.stringify(selector)});
      if (el.disabled || el.getAttribute('aria-disabled') === 'true') throw new Error('Disabled control: ' + ${JSON.stringify(selector)});
      el.focus();
      const r = el.getBoundingClientRect();
      const at = {bubbles:true,cancelable:true,view:window,button:${event === 'contextmenu' ? 2 : 0},clientX:r.x+r.width/2,clientY:r.y+r.height/2};
      for (const type of ['mousedown','mouseup',${JSON.stringify(event)}]) el.dispatchEvent(new MouseEvent(type,at));
      return true;
    })()`, window);
  }
  async fill(selector,value,window='main') {
    await until(()=>this.eval(`!!document.querySelector(${JSON.stringify(selector)})`,window),`input ${selector}`);
    await this.eval(`(() => { const el=document.querySelector(${JSON.stringify(selector)}); if(!el)throw Error('Missing input '+${JSON.stringify(selector)}); el.focus(); Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(el,${JSON.stringify(value)}); el.dispatchEvent(new Event('input',{bubbles:true}));return true; })()`,window);
  }
  count(selector) { return this.eval(`document.querySelectorAll(${JSON.stringify(selector)}).length`); }
  pressed(selector, value) { return this.eval(`document.querySelector(${JSON.stringify(selector)})?.getAttribute('aria-pressed') === ${JSON.stringify(String(value))}`); }
  async key(key, metaKey = false) {
    await this.eval(`(() => { document.activeElement?.blur(); for(const type of ['keydown','keyup']) document.body.dispatchEvent(new KeyboardEvent(type,{key:${JSON.stringify(key)},metaKey:${metaKey},bubbles:true,cancelable:true})); return true; })()`);
  }
}
async function stop() {
  app?.socket.destroy();
  app = null;
  if (!child || child.exitCode !== null) return;
  const exited = once(child, 'exit');
  child.kill('SIGTERM');
  const timer = setTimeout(() => child.kill('SIGKILL'), 5000);
  await exited;
  clearTimeout(timer);
}
async function start() {
  const server = createServer();
  server.listen(0, '127.0.0.1');
  await once(server, 'listening');
  const port = server.address().port;
  await new Promise(resolve => server.close(resolve));
  const log = openSync(path.join(out, `app-${results.length}.log`), 'a');
  child = spawn(path.join(repo, 'target/debug/rbxport'), [], {
    cwd: root, env: { ...env, RBXPORT_TEST_PORT: String(port) }, stdio: ['ignore', log, log],
  });
  closeSync(log);
  child.on('error', error => console.error(error));
  await until(async () => {
    if (child.exitCode !== null) throw new Error(`App exited: ${child.exitCode}`);
    try {
      const socket = createConnection({ host: '127.0.0.1', port });
      await new Promise((resolve, reject) => { socket.once('connect', resolve); socket.once('error', reject); });
      app = new Desktop(socket);
      return true;
    } catch { return false; }
  }, 'desktop test port', 60000);
  // Fail closed if --no-build names an ordinary or mock-backed binary.
  assert.equal(await app.eval(`!!window.__TAURI_INTERNALS__ && location.protocol === 'tauri:'`), true, 'Expected embedded Tauri frontend');
  runtime = await app.eval('({userAgent:navigator.userAgent,protocol:location.protocol,initialPreferences:localStorage.getItem("rbl.preferences")})');
  if(launches++===0)assert.equal(runtime.initialPreferences,null,'New desktop run must have an empty isolated WebKit preference store');
  await until(() => app.eval(`document.querySelector('[data-testid="browser-title"]')?.textContent.includes('Tracks)')`), 'fixture library render');
  await app.click('[role="treeitem"]', { text: 'All Tracks' });
  const count = database().tracks.length;
  await until(() => app.eval(`document.querySelector('[data-testid="browser-title"]')?.textContent.includes('(${count} Tracks)')`), 'fixture track count');
  await app.request({layout:true});
  await until(async()=>{const viewport=await app.eval('({width:innerWidth,height:innerHeight})');assert.deepEqual(viewport,{width:1200,height:800});return true;}, 'fixed 1200 × 800 viewport');
}
const deck = '[aria-label="Preview player"]';
const pad = letter => `${deck} [aria-label="Hot cues"] button[aria-label="Hot cue ${letter}"]`;
const clear = letter => `[aria-label="Cue list"] button[aria-label="Clear hot cue ${letter}"]`;
const memory = '[aria-label="Cue list"] button[aria-label^="Delete memory cue "]';
async function load(title = 'Fixture Track 0',{dual=false}={}) {
  await until(() => app.eval(`[...document.querySelectorAll('[role="gridcell"][data-col="title"]')].some(el => el.textContent.trim() === ${JSON.stringify(title)})`), `browser row ${title}`);
  await app.click('[role="gridcell"][data-col="title"]', { text: title, event: 'dblclick' });
  await until(() => app.eval(`document.querySelector('${deck} [data-testid="player-title"]')?.textContent.trim() === ${JSON.stringify(title)} && !!document.querySelector('${dual?'[aria-label="Deck A transport"]':deck} button[aria-label="Play"]:not(:disabled)')`), 'loaded playable fixture');
}
async function tab(name) { await app.click('[aria-label="Cue list"] [role="tab"]', { text: name }); }
async function editable() {
  // Test setup only: incognito storage belongs to this test window. Reload
  // mounts the real preference provider; mutations still use UI controls.
  await app.eval(`(() => { localStorage.setItem('rbl.preferences', JSON.stringify({advanced:{protectLibrary:false},view:{language:'en'}})); return true; })()`);
  await app.eval(`(() => { document.documentElement.dataset.desktopReload = 'waiting'; setTimeout(() => location.reload(), 50); return true; })()`);
  await until(() => app.eval(`!document.documentElement.dataset.desktopReload && !!document.querySelector('[data-testid="browser-title"]')`), 'reload');
  await app.click('[role="treeitem"]', { text: 'All Tracks' });
  await load();
  await tab('HOT CUE');
  await until(() => app.eval(`!!document.querySelector('${pad('A')}:not(:disabled)')`), 'writable cue controls');
}
function database(directory=library) {
  return JSON.parse(command(path.join(repo, 'target/debug/examples/e2e_fixture'), [], {
    env: { ...env, RBXPORT_DESKTOP_SNAPSHOT: directory },
  }));
}
function snapshot() { return database().cues; }
async function scenario(name, body) {
  const at = Date.now();
  console.log(`RUN ${name}`);
  try { await body(); results.push({ name, status: 'passed', ms: Date.now() - at }); console.log(`PASS ${name}`); }
  catch (error) { results.push({ name, status: 'failed', ms: Date.now() - at, error: error.stack }); throw error; }
}
try {
  console.log(`Evidence: ${out}\nTemporary library: ${library}`);
  const signature = () => createHash('sha256').update(readFileSync(path.join(repo, 'target/debug/rbxport'))).update(readFileSync(path.join(repo, 'scripts/desktop/tauri.conf.json'))).digest('hex');
  const stamp = path.join(repo, 'target/debug/desktop-test-build.sha256');
  if (!reuse) {
    command('pnpm', ['tauri', 'build', '--debug', '--no-bundle', '--config', 'scripts/desktop/tauri.conf.json'], { stdio: 'inherit' });
    command('cargo', ['build', '-q', '-p', 'rbl-db', '--example', 'e2e_fixture']);
    command('cargo',['build','-q','-p','rbl-export','--example','desktop_verify']);
    command('/usr/bin/swiftc',['scripts/desktop/native-input.swift','-o',native]);
    writeFileSync(stamp, signature());
  } else assert.equal(readFileSync(stamp, 'utf8'), signature(), 'Rebuild with pnpm desktop:test; this binary was not built by the isolated desktop harness');
  binarySha = createHash('sha256').update(readFileSync(path.join(repo, 'target/debug/rbxport'))).digest('hex');
  command(path.join(repo, 'target/debug/examples/e2e_fixture'), [library, library]);
  await until(async()=>{
    try {const fd=openSync(lockFile,'wx');writeFileSync(fd,String(process.pid));closeSync(fd);ownsLock=true;return true;}
    catch(error){if(error.code!=='EEXIST')throw error;try{const pid=Number(readFileSync(lockFile,'utf8'));if(pid>0)process.kill(pid,0);}catch(e){if(e.code==='ESRCH'){try{unlinkSync(lockFile);}catch{}}}return false;}
  },'exclusive desktop GUI test session',600000);
  await start();
  if(!expandedOnly) {
  await scenario('fresh install protects cue writes', async () => {
    await load();
    await until(() => app.eval(`document.querySelector('${pad('A')}')?.disabled === true`), 'protected empty pad');
    assert.equal(await app.eval(`document.querySelector('${deck} button[aria-label="Set memory cue"]')?.disabled`), true);
    assert.deepEqual(snapshot(), []);
  });
  await scenario('existing unlocked preferences enable cue editing', editable);
  await scenario('hot cue creation reaches SQLCipher and both waveform markers', async () => {
    await app.click(pad('A'));
    await until(() => app.pressed(pad('A'), true), 'hot cue A saved');
    for (const waveform of ['player-overview', 'player-detail']) {
      await until(async () => await app.count(`[data-testid="${waveform}"] [data-cue="A"]`) === 1, `${waveform} cue marker`);
    }
    assert.equal(snapshot().filter(c => c.track === '10000' && c.kind === 1).length, 1);
  });
  await scenario('memory cue creation reaches SQLCipher and list', async () => {
    await tab('MEMORY');
    await app.click(`${deck} button[aria-label="Set memory cue"]`);
    await until(async () => await app.count(memory) === 1, 'memory cue saved');
    assert.equal(snapshot().filter(c => c.track === '10000' && c.kind === 0).length, 1);
  });
  await scenario('cue creation survives full process restart', async () => {
    await stop(); await start(); await editable();
    await until(() => app.pressed(pad('A'), true), 'persisted hot cue');
    await tab('MEMORY');
    await until(async () => await app.count(memory) === 1, 'persisted memory cue');
  });
  await scenario('hot and memory deletion reach SQLCipher', async () => {
    await app.click(memory);
    await until(async () => await app.count(memory) === 0, 'memory cue deleted');
    await tab('HOT CUE');
    await app.click(clear('A'));
    await until(() => app.pressed(pad('A'), false), 'hot cue deleted');
    assert.deepEqual(snapshot(), []);
  });
  await scenario('keyboard cue set and clear use the real backend', async () => {
    await app.key('1'); await until(() => app.pressed(pad('A'), true), 'keyboard hot cue set');
    assert.equal(snapshot().length, 1);
    await app.key('1', true); await until(() => app.pressed(pad('A'), false), 'keyboard hot cue cleared');
    await tab('MEMORY');
    await app.key('m'); await until(async () => await app.count(memory) === 1, 'keyboard memory set');
    await app.key('x'); await until(async () => await app.count(memory) === 0, 'keyboard memory cleared');
    assert.deepEqual(snapshot(), []);
  });
  await scenario('repeated cue edits stay responsive without duplicate or stale cues', async () => {
    await tab('HOT CUE');
    for (let i = 0; i < 12; i++) {
      await app.click(pad('B'));
      await until(() => app.pressed(pad('B'), true), `iteration ${i} set`);
      await app.click(clear('B'));
      await until(() => app.pressed(pad('B'), false), `iteration ${i} clear`);
      assert.equal(await app.eval('new Promise(resolve => requestAnimationFrame(() => resolve(true)))'), true);
    }
    assert.deepEqual(snapshot(), []);
  });
  await scenario('native playback and track changes remain responsive during cue edits', async () => {
    for (let i = 0; i < 3; i++) {
      await load(`Fixture Track ${i}`); await tab('HOT CUE');
      await app.click(`${deck} button[aria-label="Play"]`);
      await until(() => app.eval(`!!document.querySelector('${deck} button[aria-label="Pause"]')`), 'native playback started');
      const elapsed = () => app.eval(`document.querySelector('${deck} [class*="elapsed"]')?.textContent`);
      const before = await elapsed();
      await until(async () => await elapsed() !== before, 'native playback advances');
      await app.click(pad('C')); await until(() => app.pressed(pad('C'), true), 'cue saved during playback');
      await app.click(clear('C')); await until(() => app.pressed(pad('C'), false), 'cue cleared during playback');
      await app.click(`${deck} button[aria-label="Pause"]`);
    }
    assert.deepEqual(snapshot(), []);
  });
  await scenario('library search completes after repeated playback and cue writes', async () => {
    const search = 'input[aria-label="Search within this track list"]';
    for (const [query, count] of [['track 010',1], ['no such fixture',0], ['',40]]) {
      await app.eval(`(() => {
        const el = document.querySelector(${JSON.stringify(search)});
        Object.getOwnPropertyDescriptor(HTMLInputElement.prototype,'value').set.call(el,${JSON.stringify(query)});
        el.dispatchEvent(new Event('input',{bubbles:true})); return true;
      })()`);
      await until(() => app.eval(`document.querySelector('[data-testid="browser-title"]')?.textContent.includes('(${count} Tracks)')`), `search returns ${count} tracks`);
    }
  });
  await scenario('deletions survive full process restart', async () => {
    await stop(); await start(); await editable();
    assert.equal(await app.pressed(pad('A'), false), true);
    await tab('MEMORY'); assert.equal(await app.count(memory), 0);
    assert.deepEqual(snapshot(), []);
  });
  } else await editable();
  await expanded({scenario,until,root,library,out,command,database,destination,native,verifier,load,start,stop,editable,app:()=>app,pid:()=>child.pid,resultCount:()=>results.length});
} catch (error) {
  failure = error.stack;
  console.error(error);
  process.exitCode = 1;
  try { if (app) writeFileSync(path.join(out, 'failure.html'), await app.eval('document.documentElement.outerHTML')); } catch { /* frozen page: process logs remain */ }
} finally {
  await stop();
  if(ownsLock)unlinkSync(lockFile);
  writeFileSync(path.join(out, 'results.json'), JSON.stringify({ platform: process.platform, macOS, arch: process.arch, host: hostname(), selection:expandedOnly?'expanded':'full', cpu: cpus()[0]?.model, runtime, binarySha, failure, root, status: process.exitCode ? 'failed' : 'passed', results }, null, 2));
  console.log(`Evidence: ${out}\nFixture retained for inspection: ${library}`);
}
