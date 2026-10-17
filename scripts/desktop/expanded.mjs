import assert from 'node:assert/strict';
import { copyFileSync, mkdirSync, writeFileSync, existsSync, readdirSync, realpathSync, statSync } from 'node:fs';
import path from 'node:path';

export async function expanded(c) {
  const { scenario, until, root, library, out, command, database, destination, native, load, start, stop, editable } = c;
  const app = () => c.app();
  const tree = '[role="treeitem"]';
  const name = 'Desktop regression playlist';
  const menu = '[role="menuitem"]';
  const deck = '[aria-label="Preview player"]';
  const title = '[data-testid="browser-title"]';
  const state = (expression, window) => app().eval(expression,window);
  async function clickText(selector,text,window='main') { await app().click(selector,{text,window}); }
  async function namedButton(text,window='main') {
    const selector = 'button';
    await state(`(() => { const el=[...document.querySelectorAll(${JSON.stringify(selector)})].find(e=>e.textContent.trim()===${JSON.stringify(text)}); if(!el || el.disabled) throw Error('Missing enabled button: '+${JSON.stringify(text)}); el.click(); return true; })()`,window);
  }
  async function waitText(selector,text,window='main') { await until(()=>state(`[...document.querySelectorAll(${JSON.stringify(selector)})].some(e=>e.textContent.includes(${JSON.stringify(text)}))`,window),text); }
  async function collection() { await clickText(tree,'All Tracks'); await waitText(title,'All Tracks ('); }
  async function context(selector,text,entry) {
    await app().click(selector,{text,event:'contextmenu'});
    await waitText(menu,entry);
    await clickText(menu,entry);
  }
  async function pressField(selector,key) { await state(`(() => { const el=document.querySelector(${JSON.stringify(selector)}); if(!el)throw Error('Missing input'); el.dispatchEvent(new KeyboardEvent('keydown',{key:${JSON.stringify(key)},bubbles:true,cancelable:true}));return true;})()`); }
  async function preferences() {
    await app().request({menu:'settings'});
    await until(async()=>(await app().request({windows:true})).includes('preferences'),'Preferences window');
    await waitText('[role="tab"]','Backups','preferences');
  }
  async function closePreferences() {
    command(native,['--close-window',String(c.pid()),'Preferences'],{timeout:15000});
    await until(async()=>!(await app().request({windows:true})).includes('preferences'),'Preferences closes');
  }
  async function nativeDrop(files,targetSelector,{text,cancel=false,ignored=false}={}) {
    command(native,['--layout',String(c.pid())],{timeout:30000});
    await until(()=>state('innerWidth===1200 && innerHeight===800'),'fixed 1200 × 800 viewport');
    await state(`(() => { window.__desktopDrops=[];window.__desktopDrags=[];window.__desktopDragListener ??= e=>window.__desktopDrags.push({trusted:e.isTrusted,types:[...e.dataTransfer.types]});document.removeEventListener('dragenter',window.__desktopDragListener,true);document.addEventListener('dragenter',window.__desktopDragListener,true); window.__desktopDropListener ??= e=>window.__desktopDrops.push({trusted:e.isTrusted,names:[...e.dataTransfer.files].map(f=>f.name),types:[...e.dataTransfer.types]}); document.removeEventListener('drop',window.__desktopDropListener,true); document.addEventListener('drop',window.__desktopDropListener,true);return true;})()`);
    const at = await state(`(() => {const el=[...document.querySelectorAll(${JSON.stringify(targetSelector)})].find(e=>${text===undefined?'true':`e.textContent.includes(${JSON.stringify(text)})`}); if(!el)throw Error('Missing native drop target'); const r=el.getBoundingClientRect(); return {x:r.x+r.width*.65,y:r.y+Math.min(r.height*.65,300),height:innerHeight};})()`);
    const evidence = JSON.parse(command(native,[String(c.pid()),String(at.x),String(at.y),String(at.height),...files,...(cancel?['--cancel']:[])],{timeout:30000}));
    const drops=await state('window.__desktopDrops');
    const drags=await state('window.__desktopDrags');
    assert.ok(drags.some(e=>e.trusted && e.types.includes('Files')),'A trusted native file drag must enter the test window');
    writeFileSync(path.join(out,`native-drop-${c.resultCount()}.json`),JSON.stringify({files,targetSelector,text,evidence,drags,drops},null,2));
    if(cancel) assert.equal(drops.length,0,'Cancelled drag must not reach a drop handler');
    else if(ignored) {assert.ok(drops.length<=1);if(drops.length)assert.equal(drops[0].trusted,true);}
    else { assert.equal(drops.length,1,'OS must deliver exactly one drop'); assert.equal(drops[0].trusted,true,'OS drop must be trusted'); assert.ok(drops[0].types.includes('Files')); }
  }
  const sources=path.join(root,`Finder Sources ${path.basename(root)}`);
  mkdirSync(sources);
  const single=path.join(sources,'Écho café 你好.wav');
  const multi=[path.join(sources,'Two tracks A.wav'),path.join(sources,'Two tracks B.wav')];
  for(const f of [single,...multi])copyFileSync(path.join(library,'audio/Fixture Track 0.wav'),f);
  // A deterministic 120 BPM click track for the analysis and loop checks.
  const rate=44100, frames=rate*16, wave=Buffer.alloc(44+frames*4);
  const template=command('/bin/cat',[single],{encoding:null});
  template.copy(wave,0,0,44);wave.writeUInt32LE(36+frames*4,4);wave.writeUInt32LE(frames*4,40);
  for(let i=0;i<frames;i++){const phase=(i+rate*.45)%(rate/2);const click=phase<rate*.015?Math.sin(2*Math.PI*2000*phase/rate)*Math.exp(-phase/(rate*.003))*.7:0;const value=Math.round((click+Math.sin(2*Math.PI*220*i/rate)*.02)*25000);wave.writeInt16LE(value,44+i*4);wave.writeInt16LE(value,46+i*4);}
  writeFileSync(single,wave);
  const canonical = p=>realpathSync(p).normalize('NFC');
  const held = p=>database().tracks.some(t=>t.path && existsSync(t.path) && canonical(t.path)===canonical(p));

  await scenario('desktop viewport is fixed across reloads and restarts',async()=> {
    assert.deepEqual(await state('({width:innerWidth,height:innerHeight})'),{width:1200,height:800});
  });
  await scenario('playlist creation rename undo and redo persist in SQLCipher',async()=> {
    await context('[role="treeitem"][data-kind="collection"]',undefined,'Create New Playlist');
    await waitText(tree,'New playlist');
    await context(tree,'New playlist','Rename Playlist');
    const input='input[aria-label="Rename New playlist"]';
    await app().fill(input,name); await pressField(input,'Enter');
    await waitText(tree,name);
    assert.ok(database().playlists.some(p=>p.name===name));
    await app().request({menu:'undo'}); await waitText(tree,'New playlist');
    assert.ok(!database().playlists.some(p=>p.name===name));
    await app().request({menu:'redo'}); await waitText(tree,name);
    assert.ok(database().playlists.some(p=>p.name===name));
  });
  await scenario('metadata rating edit undo and redo reach the database',async()=> {
    await collection();
    // Attribute columns differ by layout; the rating column is enabled from its header menu.
    await app().click('[role="columnheader"]',{text:'BPM',event:'contextmenu'});
    const ratingChoice='[role="menuitemcheckbox"]';
    const shown=await state(`[...document.querySelectorAll('${ratingChoice}')].find(e=>e.textContent.trim().endsWith('Rating'))?.getAttribute('aria-checked')==='true'`);
    if(!shown)await clickText(ratingChoice,'Rating');
    await app().key('Escape');
    await until(()=>state(`!!document.querySelector('[data-col="rating"] [aria-label="4 of 5"]')`),'rating column');
    const selector='[role="row"]:has([role="gridcell"][data-col="title"]) [data-col="rating"] [aria-label="4 of 5"]';
    const track=await state(`document.querySelector('[role="row"]:has([role="gridcell"][data-col="title"]) [data-col="title"]')?.textContent.trim()`);
    await app().click(selector);
    await until(()=>state(`document.querySelector(${JSON.stringify(selector)})?.getAttribute('aria-checked')==='true'`),'rating saved');
    await until(()=>database().tracks.find(t=>t.title===track).rating===4,'rating reaches SQLCipher');
    await app().request({menu:'undo'});
    await until(()=>state(`document.querySelector(${JSON.stringify(selector)})?.getAttribute('aria-checked')==='false'`),'rating undo');
    await until(()=>database().tracks.find(t=>t.title===track).rating===0,'rating undo reaches SQLCipher');
    await app().request({menu:'redo'});
    await until(()=>state(`document.querySelector(${JSON.stringify(selector)})?.getAttribute('aria-checked')==='true'`),'rating redo');
    await until(()=>database().tracks.find(t=>t.title===track).rating===4,'rating redo reaches SQLCipher');
  });
  await scenario('native Finder input permission is available',async()=> {
    const access=JSON.parse(command(native,['--preflight']));
    assert.equal(access.accessibility,true); assert.equal(access.postEvents,true);
  });
  await scenario('Finder drops a Unicode filename into an open playlist with its real path',async()=> {
    await clickText(tree,name); await waitText(title,'(0 Tracks)');
    await nativeDrop([single],'[data-testid="track-scroll"]');
    await waitText(title,'(1 Tracks)');
    assert.ok(held(single),'Imported path must resolve to the original Unicode file');
    await load('Écho café 你好');
  });
  await scenario('repeated Finder import reuses the track and prompts for playlist duplicates',async()=> {
    const before=database().tracks.length;
    await nativeDrop([single],'[data-testid="track-scroll"]');
    await waitText('[role="alertdialog"]','Duplicate');
    await namedButton('Skip duplicates');
    await waitText(title,'(1 Tracks)');
    assert.equal(database().tracks.length,before);
  });
  await scenario('Finder delivers multiple files directly to a playlist tree row',async()=> {
    await nativeDrop(multi,tree,{text:name});
    await waitText(title,'(3 Tracks)');
    assert.ok(multi.every(held));
  });
  await scenario('a cancelled native Finder drag changes neither library nor playlist',async()=> {
    const before=database();
    const cancelled=path.join(sources,'Cancelled.wav'); copyFileSync(single,cancelled);
    await nativeDrop([cancelled],'[data-testid="track-scroll"]',{cancel:true});
    assert.deepEqual(database(),before);
  });
  await scenario('dropping Finder audio on the toolbar is ignored without navigating away',async()=> {
    const before=database();
    const rejected=path.join(sources,'Invalid target.wav');copyFileSync(single,rejected);
    await nativeDrop([rejected],'header',{ignored:true});
    assert.equal(await state('location.protocol'),'tauri:');assert.deepEqual(database(),before);
    await waitText(title,'(3 Tracks)');
  });
  await scenario('unsupported files arrive through Finder and are rejected without data loss',async()=> {
    const unsupported=path.join(sources,'Not audio.txt');writeFileSync(unsupported,'desktop fixture');
    const before=database();
    await nativeDrop([unsupported],'[data-testid="track-scroll"]');
    await until(()=>state(`document.querySelector('footer')?.textContent.includes('Nothing') || document.querySelector('footer')?.textContent.toLowerCase().includes('skipped') || document.querySelector('footer')?.textContent.includes('supported') || document.querySelector('footer')?.textContent.includes('No tracks found')`),'unsupported import outcome');
    assert.deepEqual(database(),before);
  });
  await scenario('Finder folder drop creates a playlist and recursively imports its audio',async()=> {
    const folder=path.join(root,`Folder Drop ${path.basename(root)}`);mkdirSync(path.join(folder,'Nested'),{recursive:true});
    const file=path.join(folder,'Nested','Folder tone.wav');copyFileSync(single,file);
    await nativeDrop([folder],'[role="treeitem"][data-kind="collection"]');
    await waitText(tree,path.basename(folder));await clickText(tree,path.basename(folder));await waitText(title,'(1 Tracks)');
    assert.ok(held(file));
  });
  await scenario('missing audio is refused without replacing the loaded deck',async()=> {
    await collection();await load('Fixture Track 0');
    await app().click('[role="gridcell"][data-col="title"]',{text:'Track 003',event:'dblclick'});
    await until(()=>state(`document.querySelector('footer')?.textContent.toLowerCase().includes('missing') || document.querySelector('footer')?.textContent.toLowerCase().includes('could not be found')`),'missing audio explanation');
    assert.equal(await state(`document.querySelector('${deck} [data-testid="player-title"]')?.textContent.trim()`),'Fixture Track 0');
  });
  await scenario('cancelling desktop analysis settings leaves the library unchanged',async()=> {
    await app().fill('input[aria-label="Search within this track list"]','Écho café');
    await waitText(title,'(1 Tracks)');
    await app().click('[role="gridcell"][data-col="title"]',{text:'Écho café 你好'});
    const before=database();await app().request({menu:'analyze-track'});
    await waitText('dialog','Analysis Setting');await namedButton('Cancel');
    await until(()=>state(`!document.querySelector('dialog[open]')`),'analysis settings closes');
    assert.deepEqual(database(),before);
  });
  await scenario('analysis is launched through the desktop menu and saves readable artifacts',async()=> {
    const before=database().tracks.find(t=>t.title==='Écho café 你好').analysis;
    const oldFile=before?path.join(library,'share',before.replace(/^\//,'')):null;
    const oldTime=oldFile && existsSync(oldFile)?statSync(oldFile).mtimeMs:0;
    await app().click('[role="gridcell"][data-col="title"]',{text:'Écho café 你好'});
    await app().request({menu:'analyze-track'});
    await waitText('dialog','Analysis Setting');
    await namedButton('OK');
    await until(()=>Boolean(database().tracks.find(t=>t.title==='Écho café 你好').analysis),'analysis saved',120000);
    const analysed=database().tracks.find(t=>t.title==='Écho café 你好');
    assert.ok(analysed.bpm>=11900 && analysed.bpm<=12100,`Expected 120 BPM fixture, got ${analysed.bpm}`);
    const relative=analysed.analysis;
    const file=path.join(library,'share',relative.replace(/^\//,''));
    await until(()=>existsSync(file) && statSync(file).mtimeMs>oldTime,'analysis artifacts are freshly written',120000);
    assert.ok(existsSync(file)); assert.ok(existsSync(file.replace(/\.DAT$/i,'.EXT')));
  });
  await scenario('two native decks play independently and an analysed beat loop enters and exits',async()=> {
    await app().click('button[aria-label="Layout"]');await clickText('[role="menuitemradio"]','2 PLAYER');
    await load('Écho café 你好',{dual:true});
    await app().fill('input[aria-label="Search within this track list"]','');
    await waitText(title,`(${database().tracks.length} Tracks)`);
    await context('[role="gridcell"][data-col="title"]','Fixture Track 2','Load');
    await clickText(menu,'Load track to player 2');
    const b='[aria-label="Preview player B"]';
    await until(()=>state(`document.querySelector('${b} [data-testid="player-title"]')?.textContent.trim()==='Fixture Track 2'`),'deck B load');
    for(const region of ['[aria-label="Deck A transport"]','[aria-label="Deck B transport"]'])await app().click(`${region} button[aria-label="Play"]`);
    const before=await state(`[...document.querySelectorAll('[data-testid="player-time"]')].map(e=>e.textContent)`);
    await until(async()=>{const now=await state(`[...document.querySelectorAll('[data-testid="player-time"]')].map(e=>e.textContent)`);return now.length===2 && now.every((v,i)=>v!==before[i]);},'both native playheads advance');
    await app().click(`${deck} button[aria-label$="beat loop"]`);
    await until(()=>state(`!!document.querySelector('${deck} button[aria-label="Exit loop"]')`),'beat loop enters');
    const position=()=>state(`(() => {const text=document.querySelector('${deck} [class*="elapsed"]')?.textContent??'';const match=/(\\d+):(\\d+(?:\\.\\d+)?)/.exec(text);return match?Number(match[1])*60+Number(match[2]):null;})()`);
    let previous=await position();assert.notEqual(previous,null);
    await until(async()=>{const now=await position();assert.notEqual(now,null);const wrapped=now<previous-.2;previous=now;return wrapped;},'native deck playhead wraps inside the beat loop',6000);
    await app().click(`${deck} button[aria-label="Exit loop"]`);
    for(const region of ['[aria-label="Deck A transport"]','[aria-label="Deck B transport"]'])await app().click(`${region} button[aria-label="Pause"]`);
    await app().click('button[aria-label="Layout"]');await clickText('[role="menuitemradio"]','1 PLAYER');
  });
  await scenario('desktop export menu writes both libraries to a temporary destination',async()=> {
    await state(`(() => {window.__desktopExportStatus=[]; window.__desktopExportObserver?.disconnect(); window.__desktopExportObserver=new MutationObserver(()=>window.__desktopExportStatus.push(document.querySelector('footer')?.textContent??'')); window.__desktopExportObserver.observe(document.querySelector('footer'),{childList:true,subtree:true,characterData:true}); return true;})()`);
    await context(tree,name,'Export Playlist');
    await clickText(menu,path.basename(destination));
    await until(()=>existsSync(path.join(destination,'PIONEER/rekordbox/export.pdb')) && existsSync(path.join(destination,'PIONEER/rekordbox/exportLibrary.db')),'both export databases',60000);
    await until(()=>state(`!document.querySelector('footer progress')`),'export completes',60000);
    const checked=JSON.parse(command(c.verifier,[destination]));
    assert.equal(checked.verified,true);assert.equal(checked.snapshot.legacy.tracks.length,3);assert.equal(checked.snapshot.one.tracks.length,3);
    writeFileSync(path.join(out,'export-readback.json'),JSON.stringify(checked,null,2));
  });
  await scenario('a second desktop export preserves playable files and playlist membership',async()=> {
    await state('(() => {window.__desktopExportStatus=[];return true;})()');
    await context(tree,name,'Export Playlist');await clickText(menu,path.basename(destination));
    await until(()=>state(`window.__desktopExportStatus.some(s=>s.includes('Writing ')) && window.__desktopExportStatus.some(s=>s.includes('Updated') || s.includes('Already up to date'))`),'second export starts and reports completion',60000);
    const checked=JSON.parse(command(c.verifier,[destination]));
    assert.equal(checked.snapshot.legacy.tracks.length,3);assert.equal(checked.snapshot.one.tracks.length,3);
  });
  await scenario('Preferences creates a real backup ZIP from the disposable library',async()=> {
    await preferences();await clickText('[role="tab"]','Backups','preferences');
    await namedButton('Create backup','preferences');
    await until(()=>state(`document.querySelectorAll('table[aria-label="Library backups"] tbody tr').length===1`,'preferences'),'backup saved',60000);
    const zip=[];function scan(dir){for(const entry of readdirSync(dir,{withFileTypes:true})){const file=path.join(dir,entry.name);if(entry.isDirectory())scan(file);else if(file.endsWith('.zip'))zip.push(file);}}scan(path.join(root,'state'));
    assert.equal(zip.length,1);command('/usr/bin/unzip',['-t',zip[0]]);
    const entries=command('/usr/bin/unzip',['-Z1',zip[0]]).trim().split('\n');
    const master=entries.filter(e=>path.posix.basename(e)==='master.db');assert.equal(master.length,1);
    const readback=path.join(root,'backup-readback');mkdirSync(readback);
    writeFileSync(path.join(readback,'master.db'),command('/usr/bin/unzip',['-p',zip[0],master[0]],{encoding:null}));
    assert.deepEqual(database(readback),database(),'Backup must contain the current tracks playlists metadata and cues');
    await closePreferences();
  });
  await scenario('Library Protection propagates from a real Preferences window',async()=> {
    await collection();await load();await preferences();
    await clickText('[role="tab"]','Advanced','preferences');await clickText('[role="tab"]','Browse','preferences');
    const toggle='label';
    await app().click(toggle,{text:'Protect library edit.',window:'preferences'});
    await until(()=>state(`document.querySelector('${deck} button[aria-label="Set memory cue"]')?.disabled===true`),'protection reaches main window');
    await app().click(toggle,{text:'Protect library edit.',window:'preferences'});
    await until(()=>state(`document.querySelector('${deck} button[aria-label="Set memory cue"]')?.disabled===false`),'unlock reaches main window');
    await closePreferences();
  });
  await scenario('imported tracks playlist metadata and analysis survive a process restart',async()=> {
    const before=database();await stop();await start();
    await collection();
    await waitText(tree,name);assert.deepEqual(database(),before);
    await clickText(tree,name);await waitText(title,'(3 Tracks)');
    await load('Écho café 你好');
  });
}
