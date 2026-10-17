#!/usr/bin/env python3
"""Real app process + mock Hub: permissions, ordering, lifecycle and persistence."""
import argparse, base64, io, json, os, socket, subprocess, sys, tempfile, threading, time, wave
from datetime import datetime, timezone
from http.server import BaseHTTPRequestHandler, ThreadingHTTPServer
from pathlib import Path
from urllib.request import Request, build_opener, ProxyHandler
from urllib.error import HTTPError

parser=argparse.ArgumentParser();parser.add_argument('--binary',type=Path,required=True);args=parser.parse_args()
binary=args.binary.resolve();opener=build_opener(ProxyHandler({}))
state={'active':True,'lease':1,'events':[],'cursor':0,'renders':[],'audio':[]}
PNG=base64.b64decode('iVBORw0KGgoAAAANSUhEUgAAAAEAAAABCAQAAAC1HAwCAAAAC0lEQVR42mP8/x8AAwMCAO+aWQAAAABJRU5ErkJggg==')
class Handler(BaseHTTPRequestHandler):
    def log_message(self,*args):pass
    def reply(self,data,status=200,content_type='application/json'):
        b=data if isinstance(data,bytes) else json.dumps(data).encode();self.send_response(status);self.send_header('Content-Type',content_type);self.send_header('Content-Length',str(len(b)));self.end_headers();self.wfile.write(b)
    def do_GET(self):
        if self.headers.get('Authorization')!='Bearer hub-test-secret':return self.reply({},401)
        if self.path.startswith('/runtime/events'):
            from urllib.parse import urlparse,parse_qs
            after=int(parse_qs(urlparse(self.path).query).get('after',['0'])[0])
            time.sleep(.08);return self.reply({'events':[e for e in state['events'] if e['seq']>after],'cursor':state['cursor']})
        self.reply({'active':state['active'],'lease':state['lease'],'device':{'connected':True}})
    def do_POST(self):
        if self.headers.get('Authorization')!='Bearer hub-test-secret':return self.reply({},401)
        body=self.rfile.read(int(self.headers['Content-Length']))
        if self.path=='/runtime/audio':
            if not state['active'] or int(self.headers['x-lease'])!=state['lease']:return self.reply({},409)
            with wave.open(io.BytesIO(body)) as audio:
                assert (audio.getnchannels(),audio.getsampwidth(),audio.getframerate())==(1,2,16000)
                assert 4000<audio.getnframes()<16000
            state['audio'].append({'volume':int(self.headers['x-volume']),'bytes':len(body)})
            return self.reply({'result':'Ok(Completed)'})
        v=json.loads(body)
        if v['present'] and (not state['active'] or v['lease']!=state['lease']):return self.reply({},409)
        assert len(v['layout']['elements'])<=200
        state['renders'].append(v);self.reply(PNG,content_type='image/png')
def call(url,path,data=None,headers=None):
    h=headers or {};b=None if data is None else json.dumps(data).encode()
    if b is not None:h={**h,'Content-Type':'application/json'}
    try:
        with opener.open(Request(url+path,data=b,headers=h),timeout=5) as r:return r.status,json.loads(r.read())
    except HTTPError as e:
        raw=e.read()
        try:raw=json.loads(raw)
        except:raw=raw.decode()
        return e.code,raw
def wait(fn,seconds=12):
    until=time.monotonic()+seconds
    while time.monotonic()<until:
        try:
            v=fn()
            if v:return v
        except (OSError,KeyError,TypeError):pass
        time.sleep(.1)
    raise AssertionError('Timed out')
def free_port():
    with socket.socket() as s:s.bind(('127.0.0.1',0));return s.getsockname()[1]
server=ThreadingHTTPServer(('127.0.0.1',0),Handler);threading.Thread(target=server.serve_forever,daemon=True).start()
checks=[]
with tempfile.TemporaryDirectory(prefix='agent-pet-test-') as folder:
    root=Path(folder);data=root/'data';sessions=root/'sessions';sessions.mkdir();log=(root/'app.log').open('w')
    port=free_port();url=f'http://127.0.0.1:{port}'
    env={**os.environ,'ZECTRIX_HUB_URL':f'http://127.0.0.1:{server.server_port}/','ZECTRIX_APP_TOKEN':'hub-test-secret','ZECTRIX_APP_DATA':str(data),'ZECTRIX_APP_PORT':str(port),'ZECTRIX_CODEX_SESSIONS':str(sessions)}
    process=None
    def launch():return subprocess.Popen([str(binary)],env=env,stdout=log,stderr=log)
    try:
        process=launch();boot=wait(lambda:call(url,'/api/boot')[1]);browser={'X-App-Token':boot['token']}
        assert call(url,'/api/status')[1]['settings']['codex_enabled'] is True
        wait(lambda:call(url,'/api/status')[1]['health']['codex']=='watching')
        conn=json.loads((data/'agent-connection.json').read_text());agent={'Authorization':'Bearer '+conn['token']}
        assert 'hub-test-secret' not in json.dumps(boot);assert 'token' not in call(url,'/api/status')[1];assert (data/'agent-connection.json').stat().st_mode&0o777==0o600
        event={'task_id':'test-1','seq':1,'status':'working','title':'测试任务','agent':'Test','progress':None}
        assert call(url,'/api/events',event)[0]==401
        assert call(url,'/api/events',event,{**agent,'Origin':'https://foreign.example'})[0]==403
        assert call(url,'/api/settings',{'codex_enabled':True,'animation':True,'stale_minutes':15},agent)[0]==401
        assert call(url,'/api/status',headers={'Host':'foreign.example'})[0]==403
        checks.append('credential isolation, cross-origin and Host checks')
        assert call(url,'/api/events',event,agent)[1]['applied']
        assert not call(url,'/api/events',event,agent)[1]['applied']
        assert call(url,'/api/events',{**event,'seq':2,'progress':101},agent)[0]==400
        subprocess.run([str(binary),'send','--connection',str(data/'agent-connection.json'),'--task','test-1','--seq','2','--state','waiting','--title','等待确认'],check=True,stdout=subprocess.DEVNULL)
        assert call(url,'/api/status')[1]['current']['event']['status']=='waiting'
        wait(lambda:len(state['audio'])==1)
        assert state['audio'][0]['volume']==90
        assert call(url,'/api/events',{**event,'seq':3,'status':'done'},agent)[0]==200
        wait(lambda:len(state['audio'])==2)
        assert all(x['volume']==90 and 8000<x['bytes']<32000 for x in state['audio'])
        assert not call(url,'/api/events',{**event,'seq':3,'status':'done'},agent)[1]['applied']
        time.sleep(2.2);assert len(state['audio'])==2
        assert call(url,'/api/settings',{'volume':101},browser)[0]==400
        checks.append('distinct transition chimes at 90%, duplicate suppression, volume validation')
        assert call(url,'/api/events',{**event,'seq':4},agent)[0]==400
        checks.append('real CLI ingestion, idempotence, progress validation, terminal-state protection')
        for status in ['working','waiting','done','error','unknown']:
            assert call(url,'/api/demo',{'status':status},browser)[0]==200
            label={'working':'正在工作','waiting':'等你处理','done':'任务完成','error':'遇到问题','unknown':'状态待确认'}[status]
            wait(lambda:state['renders'] and label in json.dumps(state['renders'][-1],ensure_ascii=False))
        assert len(call(url,'/api/status')[1]['tasks'])==1
        assert len(state['audio'])==2, 'Demo must remain silent'
        call(url,'/api/demo',{'status':None},browser)
        state['active']=False;state['lease']=2;wait(lambda:not call(url,'/api/status')[1]['runtime']['active']);time.sleep(3)
        start=len(state['renders']);time.sleep(3);assert all(not r['present'] for r in state['renders'][start:])
        state['active']=True;state['lease']=3;wait(lambda:call(url,'/api/status')[1]['runtime']['active'])
        wait(lambda:any(r['lease']==3 and r['present'] for r in state['renders']))
        state['events'].append({'seq':1,'lease':1,'event':{'type':'key','key':'Down','action':'Short'}});state['cursor']=1
        time.sleep(.4);assert call(url,'/api/status')[1]['selected'] is None
        state['events'].append({'seq':2,'lease':3,'event':{'type':'key','key':'Down','action':'Short'}});state['cursor']=2
        wait(lambda:call(url,'/api/status')[1]['selected']=='agent:test-1')
        state['events'].append({'seq':3,'lease':3,'event':{'type':'key','key':'Confirm','action':'Short'}});state['cursor']=3
        wait(lambda:call(url,'/api/status')[1]['selected'] is None)
        checks.append('inactive app does not present; reactivation uses current lease')
        task=lambda kind:{'type':'event_msg','timestamp':datetime.now(timezone.utc).isoformat(),'payload':{'type':kind,'turn_id':'codex-turn-1'}}
        session=sessions/'session.jsonl';session.write_text(json.dumps(task('task_started'))+'\n')
        wait(lambda:any(t['source']=='codex' and t['event']['status']=='working' for t in call(url,'/api/status')[1]['tasks']))
        with session.open('a') as f:f.write(json.dumps(task('task_complete'))+'\n')
        wait(lambda:any(t['source']=='codex' and t['event']['status']=='done' for t in call(url,'/api/status')[1]['tasks']))
        checks.append('fresh install follows Codex without a settings change; explicit completion')
        audio_before_restart=len(state['audio'])
        process.terminate();process.wait(timeout=6);assert not (data/'agent-connection.json').exists()
        process=launch();boot2=wait(lambda:call(url,'/api/boot')[1]);assert boot2['token']!=boot['token'];assert len(call(url,'/api/status')[1]['tasks'])==2
        assert call(url,'/api/events',{**event,'task_id':'new'},agent)[0]==401
        time.sleep(3);assert len(state['audio'])==audio_before_restart, 'History must not replay chimes'
        checks.append('restart preserves tasks/settings, rotates credentials, no historical chimes')
        for command,expect in [(['/usr/bin/true'],0),(['/usr/bin/false'],1)]:
            result=subprocess.run([str(binary),'run','--connection',str(data/'agent-connection.json'),'--title','命令测试','--',*command]);assert result.returncode==expect
        statuses=[t['event']['status'] for t in call(url,'/api/status')[1]['tasks']];assert 'error' in statuses
        checks.append('command wrapper reports successful and failing exits')
        browser={'X-App-Token':boot2['token']}
        conn=json.loads((data/'agent-connection.json').read_text());agent={'Authorization':'Bearer '+conn['token']}
        time.sleep(2.3);before=len(state['audio'])
        state['active']=False;state['lease']=4;wait(lambda:not call(url,'/api/status')[1]['runtime']['active'])
        call(url,'/api/events',{**event,'task_id':'silent-background','status':'waiting'},agent)
        time.sleep(2.3);assert len(state['audio'])==before
        state['active']=True;state['lease']=5;wait(lambda:call(url,'/api/status')[1]['runtime']['active'])
        time.sleep(2.3);assert len(state['audio'])==before
        call(url,'/api/settings',{'codex_enabled':True,'animation':True,'stale_minutes':15,'sound_enabled':False,'volume':35},browser)
        call(url,'/api/events',{**event,'task_id':'silent-disabled','status':'done'},agent)
        time.sleep(2.3);assert len(state['audio'])==before
        call(url,'/api/settings',{'codex_enabled':True,'animation':True,'stale_minutes':15,'sound_enabled':True,'volume':35},browser)
        call(url,'/api/events',{**event,'task_id':'adjusted-volume','status':'waiting'},agent)
        wait(lambda:len(state['audio'])==before+1);assert state['audio'][-1]['volume']==35
        checks.append('background and disabled states stay silent; volume changes reach Hub')
        (root/'session_index.jsonl').write_text(json.dumps({'id':'approval-chat','thread_name':'Approval task name'})+'\n')
        approval_session=sessions/'approval.jsonl'
        approval_session.write_text(json.dumps({'type':'session_meta','payload':{'id':'approval-chat','cwd':'/test'}})+'\n'+json.dumps({'type':'event_msg','timestamp':datetime.now(timezone.utc).isoformat(),'payload':{'type':'task_started','turn_id':'approval-turn'}})+'\n')
        wait(lambda:any(t['event']['task_id']=='approval-turn' for t in call(url,'/api/status')[1]['tasks']))
        hook=Path(__file__).resolve().parents[1]/'scripts/approval_hook.py'
        def hook_call(kind,command='private-command-secret'):
            payload={'hook_event_name':kind,'session_id':'approval-chat','turn_id':'approval-turn','tool_name':'Bash','tool_input':{'command':command}}
            result=subprocess.run([sys.executable,str(hook),str(data/'agent-connection.json')],input=json.dumps(payload),text=True,capture_output=True,env={**os.environ,'CODEX_HOME':str(root)})
            assert result.returncode==0 and result.stdout=='' and result.stderr==''
        time.sleep(2.3);before=len(state['audio'])
        hook_call('PermissionRequest');wait(lambda:len(state['audio'])==before+1)
        current=call(url,'/api/status')[1]['current'];assert current['event']['title']=='Approval task name';assert current['event']['status']=='waiting';assert current['event']['detail'].startswith('权限审批')
        hook_call('PermissionRequest');time.sleep(2.3);assert len(state['audio'])==before+1
        with approval_session.open('a') as f:f.write(json.dumps({'type':'event_msg','timestamp':datetime.now(timezone.utc).isoformat(),'payload':{'type':'token_count'}})+'\n')
        time.sleep(2.3);assert call(url,'/api/status')[1]['current']['event']['status']=='waiting'
        hook_call('PostToolUse');wait(lambda:next(t for t in call(url,'/api/status')[1]['tasks'] if t['event']['task_id']=='approval-turn')['event']['status']=='working')
        hook_call('PermissionRequest');wait(lambda:len(state['audio'])==before+2)
        hook_call('Interrupt');assert next(t for t in call(url,'/api/status')[1]['tasks'] if t['event']['task_id']=='approval-turn')['event']['status']=='working'
        assert 'private-command-secret' not in (data/'state.json').read_text()
        assert 'private-command-secret' not in (data/'approval-hook-state.json').read_text()
        assert call(url,'/api/approvals',{'event':'PermissionRequest'})[0]==401
        checks.append('official-hook adapter: title, chime, dedup, heartbeat overlay, resolution, no approval decisions or command leakage')
        settings=call(url,'/api/status')[1]['settings']
        assert call(url,'/api/settings',{**settings,'codex_enabled':False},browser)[0]==200
        process.terminate();process.wait(timeout=6)
        process=launch();wait(lambda:call(url,'/api/boot')[1])
        wait(lambda:call(url,'/api/status')[1]['health']['codex']=='disabled')
        disabled=call(url,'/api/status')[1]
        assert disabled['settings']['codex_enabled'] is False
        assert all(t['source']!='codex' for t in disabled['tasks'])
        checks.append('explicit Codex opt-out persists across restart')
        print(json.dumps({'passed':len(checks),'checks':checks},ensure_ascii=False,indent=2))
    finally:
        if process and process.poll() is None:process.terminate();process.wait(timeout=6)
        server.shutdown();log.close()
