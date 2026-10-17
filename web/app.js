'use strict';
const $=id=>document.getElementById(id);
const labels={working:'正在工作',waiting:'等你处理',done:'任务完成',error:'遇到问题',stopped:'已停止',unknown:'状态待确认'};
const symbols={working:'✦',waiting:'◷',done:'✓',error:'!',stopped:'□',unknown:'?'};
let token='',state=null,toastTimer;
function toast(message){$('toast').textContent=message;$('toast').hidden=false;clearTimeout(toastTimer);toastTimer=setTimeout(()=>$('toast').hidden=true,3500);}
async function api(path,data){const r=await fetch(path,data===undefined?{}:{method:'POST',headers:{'Content-Type':'application/json','X-App-Token':token},body:JSON.stringify(data)});if(!r.ok){let message=await r.text();try{message=JSON.parse(message).error||message;}catch{}throw Error(message);}return r.json();}
async function mutate(path,data){try{await api(path,data);await refresh();}catch(e){toast(e.message);}}
function tab(name){document.querySelectorAll('.tab').forEach(b=>{const active=b.dataset.tab===name;b.classList.toggle('active',active);b.setAttribute('aria-selected',String(active));});document.querySelectorAll('.tab-content').forEach(s=>s.hidden=s.id!=='tab-'+name);}
document.querySelectorAll('[data-tab]').forEach(b=>b.onclick=()=>tab(b.dataset.tab));
$('go-connect').onclick=()=>tab('connect');
$('auto-follow').onclick=()=>mutate('/api/select',{key:null});
function effective(t){return t.stale&&!['done','error','stopped'].includes(t.event.status)?'unknown':t.event.status;}
function taskLabel(t){return effective(t)==='waiting'&&t.event.detail.startsWith('权限审批')?'等待权限审批':labels[effective(t)];}
function source(t){return t.source==='codex'?'Codex 本机':t.source==='demo'?'演示':t.event.agent||'Agent 上报';}
function timestamp(t){return new Date(t*1000).toLocaleString('zh-CN',{month:'2-digit',day:'2-digit',hour:'2-digit',minute:'2-digit'});}
function idleHint(s){if(!s.settings.codex_enabled)return '本机 Codex 跟随已关闭，可在「接入 Agent」中开启，或让其他 Agent 上报状态。';if(s.health.codex==='watching')return '已自动跟随本机 Codex。开始一个任务后，状态会显示在这里。';if(!s.health.codex||s.health.codex==='disabled')return '正在连接本机 Codex，请稍候。';return '暂未接入本机 Codex：'+s.health.codex+'。也可在「接入 Agent」中配置其他 Agent。';}
function render(s){
 state=s;const t=s.current,st=t?effective(t):null;
 $('current-state').textContent=t?taskLabel(t):'等待任务';$('status-icon').textContent=symbols[st]||'○';
 const many=!s.demo&&s.running.length>1;
 $('current-title').textContent=many?s.running.length+' 个任务正在进行':t?t.event.title:s.settings.codex_enabled?'Codex 跟随已开启':'本机 Codex 跟随已关闭';
 const running=$('running-tasks');running.replaceChildren();running.hidden=!many;
 if(many)for(const task of s.running){const row=document.createElement('div');const label=document.createElement('small');label.textContent=taskLabel(task);const name=document.createElement('span');name.textContent=task.event.title;row.append(label,name);running.append(row);}
 $('current-detail').hidden=many;
 $('current-detail').textContent=t?(st==='unknown'?'一段时间未收到更新，请回到 Agent 确认。':t.event.detail||'等待 Agent 的下一次更新。'):idleHint(s);
 $('source-tag').textContent=t?source(t):s.settings.codex_enabled?'Codex 本机':'尚未接入';$('mode-badge').textContent=s.demo?'演示模式':'实时状态';
 $('auto-follow').textContent=s.selected?'回到自动跟随':'正在自动跟随';
 const p=t?.event.progress;$('progress-section').hidden=many||p===null||p===undefined;$('progress').value=p??0;$('progress-number').textContent=(p??0)+'%';
 $('updated').textContent=t?'最近更新 '+timestamp(t.updated_at):'还没有收到任务';
 const good=s.health.hub==='ready';$('connection').textContent=!good?'Hub 连接中断':s.runtime.connected?'设备已连接':'设备未连接';$('connection-dot').classList.toggle('online',good&&s.runtime.connected);
 const hub=new URL(s.runtime.hub_url);if(hub.protocol==='http:'&&['localhost','127.0.0.1'].includes(hub.hostname))$('hub-link').href=hub.href;
 $('preview-note').textContent=!good?'等待恢复 Hub 连接':s.runtime.active?(s.runtime.connected?'当前画面由 Hub 推送到设备':'当前为前台应用，连接设备后显示'):'应用在后台，仅更新预览；在 Hub 中切换到此应用';
 const errors=[];if(!good)errors.push('Hub 连接中断，正在重试。');if(s.health.render!=='ready'&&s.health.render!=='loading')errors.push('画面暂未送达：'+s.health.render);if(s.health.sound?.error)errors.push('提示音：'+s.health.sound.error);if(s.health.storage)errors.push('状态保存失败：'+s.health.storage);
 $('alert').hidden=!errors.length;$('alert').textContent=errors.join(' ');
 $('codex-enabled').checked=s.settings.codex_enabled;$('animation').checked=s.settings.animation;$('stale-minutes').value=String(s.settings.stale_minutes);$('sound-enabled').checked=s.settings.sound_enabled;if(document.activeElement!==$('volume'))$('volume').value=s.settings.volume;$('volume-value').textContent=$('volume').value+'%';$('sound-health').textContent=s.health.sound?.error?'播放未完成：'+s.health.sound.error:'';
 $('codex-health').textContent=s.health.codex==='disabled'?'尚未开启':s.health.codex==='watching'?'正在跟随本机会话':s.health.codex;
 $('task-count').textContent=s.tasks.length;
 const list=$('tasks');
 if(s.tasks.length){list.replaceChildren();for(const task of s.tasks){const key=task.source+':'+task.event.task_id;const b=document.createElement('button');b.className='task-row';b.classList.toggle('selected',t&&t.source===task.source&&t.event.task_id===task.event.task_id);b.setAttribute('aria-label','显示任务：'+task.event.title);const data=[['state',taskLabel(task)],['title',task.event.title],['source',source(task)],['date',timestamp(task.updated_at)]];for(const [cls,value]of data){const span=document.createElement('span');span.className=cls;span.textContent=value;b.append(span);}b.onclick=()=>mutate('/api/select',{key});list.append(b);}}
 else{list.replaceChildren();const d=document.createElement('div');d.className='empty';d.textContent='还没有任务。'+idleHint(s);list.append(d);}
 $('command').textContent=command();
}
function quote(s){return "'"+s.replaceAll("'","'\\''")+"'";}
function command(){if(!state)return '';return `${quote(state.executable)} send \\\n  --connection ${quote(state.connection_file)} \\\n  --task my-task-001 --seq 1 --state working \\\n  --title '整理今天的待办' --agent 'My Agent'`;}
function instructions(){return `请将本次任务的真实状态同步到 ZECTRIX Agent 宠物。应用须在 Hub 中保持运行。\n\n${command()}\n\n请把 my-task-001 替换为本次任务唯一 ID，并使用真实标题。开始使用 working；明确需要用户处理时使用 waiting；收到用户回复后恢复 working；完成使用 done；失败使用 error；取消使用 stopped。每次上报保持 task 不变，将 seq 递增为 2、3……。任务进行中至少每分钟上报一次心跳。可用 --detail 提供简短说明；只有可计算的真实进度才传 --progress 0–100，未知时省略。结束后重新开始必须使用新 task ID。不要发送密码、令牌、个人资料、对话或代码内容。上报成功不代表用户已经看到设备。`}
$('copy-instructions').onclick=async()=>{try{await navigator.clipboard.writeText(instructions());toast('接入说明已复制');}catch{toast('无法访问剪贴板，请展开命令后手动复制');}};
for(const id of ['codex-enabled','animation','stale-minutes','sound-enabled','volume'])$(id).onchange=async()=>{if(!state)return;const settings={codex_enabled:$('codex-enabled').checked,animation:$('animation').checked,stale_minutes:Number($('stale-minutes').value),sound_enabled:$('sound-enabled').checked,volume:Number($('volume').value)};$(id).disabled=true;await mutate('/api/settings',settings);$(id).disabled=false;};
document.querySelectorAll('[data-demo]').forEach(b=>b.onclick=()=>mutate('/api/demo',{status:b.dataset.demo}));$('end-demo').onclick=()=>mutate('/api/demo',{status:null});
async function refresh(){render(await api('/api/status'));}
async function start(){try{token=(await api('/api/boot')).token;await refresh();}catch(e){$('alert').hidden=false;$('alert').textContent='应用连接失败：'+e.message;}}
async function poll(){try{await refresh();$('screen').src='/screen.png?t='+Date.now();}catch(e){$('connection').textContent='应用连接中断';$('connection-dot').classList.remove('online');}finally{setTimeout(poll,3000);}}
start().then(poll);

$('volume').oninput=()=>{$('volume-value').textContent=$('volume').value+'%';};
for(const kind of ['waiting','done'])$('sound-'+kind).onclick=async()=>{const b=$('sound-'+kind);b.disabled=true;await mutate('/api/sound',{status:kind});b.disabled=false;};
