"""Independent isolated acceptance. Run on Mac mini, never installed entry."""
import argparse, http.server, json, os, pathlib, select, shlex, signal, struct, subprocess, tempfile, threading, time

parser = argparse.ArgumentParser()
parser.add_argument('entry')
parser.add_argument('--codex', default=str(pathlib.Path.home()/'.local/bin/codex'))
parser.add_argument('--case', default='contention', choices=['contention','reconnect','isolation','expiry','lifetime','crash','budget','offline','startup-crash','idle-active','idle-empty','idle-thread','idle-thread-stubborn','hard-fd','server-crash','stop-isolation','approval','user-input','guardian-crash'])
args = parser.parse_args()
release = threading.Event()
received = threading.Event()
requests = 0

class Mock(http.server.BaseHTTPRequestHandler):
    def log_message(self, *args): pass
    def do_POST(self):
        global requests
        self.rfile.read(int(self.headers.get('Content-Length', '0')))
        requests += 1
        received.set()
        if args.case not in ['approval','user-input'] or requests>1: release.wait(60)
        self.send_response(200)
        self.send_header('Content-Type','text/event-stream')
        self.end_headers()
        try:
            if args.case in ['approval','user-input'] and requests==1:
                item={'type':'function_call','id':'fc_approval','call_id':'call_approval','name':'exec_command','arguments':json.dumps({'cmd':'printf isolated-approval-probe','sandbox_permissions':'require_escalated','justification':'Isolated approval probe. Cancel without executing.'})}
                if args.case=='user-input':
                    item.update(name='request_user_input',arguments=json.dumps({'questions':[{'id':'choice','header':'Probe','question':'Choose a no-op outcome','options':[{'label':'Skip','description':'Do nothing'},{'label':'Cancel','description':'Do nothing either'}]}]}))
                event={'type':'response.output_item.done','output_index':0,'item':item}
                self.wfile.write(('event: response.output_item.done\ndata: '+json.dumps(event)+'\n\n').encode())
            self.wfile.write(b'event: response.completed\ndata: {"type":"response.completed","response":{"id":"acceptance","status":"completed","output":[],"usage":{"input_tokens":1,"output_tokens":0,"total_tokens":1}}}\n\n')
        except OSError: pass

tmp = pathlib.Path(tempfile.mkdtemp(prefix='macc-',dir='/tmp'))
root = tmp/'managed'
home = tmp/'home'
real = home/'.codex'
srv = http.server.ThreadingHTTPServer(('localhost',0), Mock)
threading.Thread(target=srv.serve_forever,daemon=True).start()
def write(path, value):
    path.parent.mkdir(parents=True,exist_ok=True)
    path.write_text(value)

mcp_log=tmp/'mcp-events.jsonl'
mcp_stub=tmp/'mcp-probe.py'
if args.case in ['idle-thread','idle-thread-stubborn']:
    write(mcp_stub, '''import json,os,signal,sys,time
log=sys.argv[1]
def event(name):
    with open(log,'a') as f: f.write(json.dumps({'event':name,'pid':os.getpid()})+'\\n')
def stop(signum,frame): event('stopped'); raise SystemExit(0)
signal.signal(signal.SIGTERM,stop); event('started')
for line in sys.stdin:
    request=json.loads(line); request_id=request.get('id'); method=request.get('method')
    if request_id is None: continue
    if method=='initialize': result={'protocolVersion':'2025-06-18','capabilities':{'tools':{'listChanged':False}},'serverInfo':{'name':'acceptance-probe','version':'1'}}
    elif method=='tools/list': result={'tools':[]}
    elif method=='resources/list': result={'resources':[]}
    elif method=='resources/templates/list': result={'resourceTemplates':[]}
    else: result={}
    print(json.dumps({'jsonrpc':'2.0','id':request_id,'result':result}),flush=True)
''')
    if args.case=='idle-thread-stubborn':
        mcp_stub.write_text(mcp_stub.read_text().replace("signal.signal(signal.SIGTERM,stop)","signal.signal(signal.SIGTERM,signal.SIG_IGN)"))
mcp_config = f'''\n[mcp_servers.acceptance_probe]\ncommand="/usr/bin/python3"\nargs=["{mcp_stub}","{mcp_log}"]\nstartup_timeout_sec=10\n''' if args.case in ['idle-thread','idle-thread-stubborn'] else ''
write(real/'config.toml', f'''model="probe"
model_provider="probe"
[model_providers.probe]
name="isolated acceptance mock"
base_url="http://localhost:{srv.server_port}/v1"
wire_api="responses"
env_key="ACCEPTANCE_FAKE_KEY"
{mcp_config}''')
real_config = (real/'config.toml').read_bytes()
(real/'computer-use/Codex Computer Use.app').mkdir(parents=True)
resources = tmp/'resources'
for name in ['codex','cua_node/bin/node','cua_node/bin/node_repl']:
    write(resources/name, 'fixture')
(resources/'cua_node/lib/node_modules').mkdir(parents=True)
for market in ['personal','openai-bundled']:
    cache = real/'plugins/cache'/market
    cache.mkdir(parents=True)
    if market == 'openai-bundled':
        plugin = cache/'unified-computer-use/1'
        write(plugin/'.codex-plugin/plugin.json','{"name":"unified-computer-use"}')
        write(plugin/'.mcp.json','{"mcpServers":{"cua_repl":{"command":"/usr/bin/false","args":[],"enabled":false,"startup_timeout_sec":1}}}')

env = os.environ.copy()
wrapper=tmp/'codex-wrapper'
delay='if [ "$2" != "proxy" ]; then sleep 9; fi\n' if args.case=='startup-crash' else ''
if args.case=='server-crash':
    delay='if [ "$2" != "proxy" ]; then /usr/bin/python3 -c '+"'import os,time;os.setsid();open(\""+str(tmp/'detached-child.pid')+"\",\"w\").write(str(os.getpid()));time.sleep(120)' & fi\n"
write(wrapper, '#!/bin/sh\n'+delay+'exec '+shlex.quote(args.codex)+' "$@" 2>>'+str(tmp/'codex-stderr.log')+'\n')
wrapper.chmod(0o700)
env.update(HOME=str(home),CODEX_HOME=str(real),CODEX_MANAGED_ROOT=str(root),
    CODEX_MANAGED_CODEX_BIN=str(wrapper),
    CODEX_MANAGED_DESKTOP_RESOURCES=str(resources),CODEX_MANAGED_TAKEOVER='false',
    CODEX_MANAGED_EOF_GRACE_SECS='3' if args.case=='expiry' else '8',
    CODEX_MANAGED_TERM_GRACE_SECS='1',CODEX_MANAGED_SESSION_MAX_SECS='6' if args.case=='lifetime' else '60',
    CODEX_MANAGED_DETACH_BUDGET_SECS='2' if args.case=='budget' else '30',CODEX_MANAGED_IDLE_SECS='120',
    ACCEPTANCE_FAKE_KEY='not-a-real-key',
    SSH_ORIGINAL_COMMAND="printf '%b' '\\001\\002\\003\\004\\005\\006\\007\\010'; PATH=whatever; export PATH; exec codex app-server proxy")
if args.case.startswith('idle-') or args.case in ['approval','user-input']:
    env.update(CODEX_MANAGED_IDLE_SECS='2',CODEX_MANAGED_DRAIN_SECS='1')
if args.case=='hard-fd':
    env.update(CODEX_MANAGED_SAMPLE_SECS='1',CODEX_MANAGED_FD_WARN='1',CODEX_MANAGED_FD_RECYCLE='2',CODEX_MANAGED_FD_HARD='3')
children = []
startup_pids=[]
owned_pids=set()
def start(identity):
    p = subprocess.Popen([args.entry,'--client-id',identity],env=env,stdin=subprocess.PIPE,stdout=subprocess.PIPE,stderr=subprocess.PIPE,start_new_session=True)
    children.append(p)
    return p

def exact(p,n,timeout=15):
    data=b''; deadline=time.monotonic()+timeout
    while len(data)<n:
        left=deadline-time.monotonic()
        if left<=0 or not select.select([p.stdout],[],[],max(0,left))[0]: raise TimeoutError('entry output timeout')
        v=os.read(p.stdout.fileno(),n-len(data))
        if not v: raise EOFError('entry exited: '+p.stderr.read().decode())
        data+=v
    return data

class Client:
    def __init__(self,p):
        self.p=p; self.n=0; self.other=[]
        assert exact(p,8)==bytes(range(1,9)), 'nonce corruption'
        p.stdin.write(b'GET / HTTP/1.1\r\nHost: localhost\r\nUpgrade: websocket\r\nConnection: Upgrade\r\nSec-WebSocket-Key: dGhlIHNhbXBsZSBub25jZQ==\r\nSec-WebSocket-Version: 13\r\n\r\n'); p.stdin.flush()
        h=b''
        while not h.endswith(b'\r\n\r\n'): h+=exact(p,1)
        assert b'101' in h, h
        self.call('initialize',{'clientInfo':{'name':'independent-acceptance','version':'1'},'capabilities':{'experimentalApi':True}})
        self.send({'method':'initialized','params':{}})
        capture_resources()
    def send(self,v):
        payload=json.dumps(v).encode(); n=len(payload); mask=os.urandom(4)
        head=bytes([129,128|n]) if n<126 else bytes([129,254])+struct.pack('!H',n)
        self.p.stdin.write(head+mask+bytes(b^mask[i%4] for i,b in enumerate(payload))); self.p.stdin.flush()
    def read(self):
        a,b=exact(self.p,2); n=b&127
        if n==126: n=struct.unpack('!H',exact(self.p,2))[0]
        if n==127: n=struct.unpack('!Q',exact(self.p,8))[0]
        return json.loads(exact(self.p,n))
    def call(self,m,params):
        self.n+=1; self.send({'id':self.n,'method':m,'params':params})
        while True:
            v=self.read()
            if v.get('id')==self.n:
                assert 'error' not in v, v
                return v['result']
            self.other.append(v)
    def close(self):
        self.p.stdin.close()
        self.p.stdout.close()

def events():
    path=root/'log/supervisor.jsonl'
    return [json.loads(v) for v in path.read_text().splitlines()] if path.exists() else []
def workers():
    return [v['pid'] for v in events() if v.get('event')=='worker_started']
def alive(pid):
    out=subprocess.run(['ps','-o','stat=','-p',str(pid)],capture_output=True,text=True).stdout.strip()
    return bool(out) and not out.startswith('Z')
def fd_counts(pid):
    details=subprocess.run(['/usr/sbin/lsof','-n','-P','-a','-p',str(pid)],capture_output=True,text=True).stdout.splitlines()
    return {'fd':max(0,len(details)-1),'pipe':sum('PIPE' in line for line in details)}
def owner_ancestor(pid):
    for _ in range(5):
        row=subprocess.run(['ps','-o','ppid=,args=','-p',str(pid)],capture_output=True,text=True).stdout.strip()
        if not row: raise AssertionError('owner ancestor disappeared')
        parent,command=row.split(None,1)
        if '--session-owner' in command: return pid
        pid=int(parent)
    raise AssertionError('session owner absent from worker ancestry')
def capture_resources():
    table=subprocess.run(['ps','-axo','pid=,ppid=,pgid='],capture_output=True,text=True).stdout
    rows=[tuple(map(int,line.split())) for line in table.splitlines()]
    for worker in workers():
        descendants={worker}|{pid for pid,parent,group in rows if group==worker}
        while True:
            previous=len(descendants)
            descendants.update(pid for pid,parent,group in rows if parent in descendants)
            if len(descendants)==previous: break
        owned_pids.update(descendants)
        current=worker
        for _ in range(4):
            row=subprocess.run(['ps','-o','ppid=,args=','-p',str(current)],capture_output=True,text=True).stdout.strip()
            if not row: break
            parent,command=row.split(None,1)
            owned_pids.add(current)
            if '--session-owner' in command: break
            current=int(parent)
            if current<=1: break
        details=subprocess.run(['/usr/sbin/lsof','-n','-P','-a','-p',str(worker)],capture_output=True,text=True).stdout
        print(json.dumps({'resourceSample':worker,'fdRows':max(0,len(details.splitlines())-1),'pipeRows':sum('PIPE' in line for line in details.splitlines()),'descendants':sorted(descendants)}),flush=True)

try:
    if args.case=='startup-crash':
        p=start('laptop')
        deadline=time.monotonic()+5
        while time.monotonic()<deadline:
            table=subprocess.run(['ps','-axo','pid=,ppid=,pgid=,args='],capture_output=True,text=True).stdout
            rows=[line.split(None,3) for line in table.splitlines() if str(wrapper) in line]
            if rows: break
            time.sleep(.05)
        assert rows, 'startup worker not found'
        worker,parent,group=map(int,rows[0][:3]); startup_pids.append(group)
        owner=owner_ancestor(worker)
        os.kill(owner,signal.SIGKILL)
        time.sleep(2)
        print(json.dumps({'startupWorker':worker,'killedOwner':owner,'workerStillAlive':alive(worker)}),flush=True)
        assert not alive(worker), 'owner death before socket readiness leaked worker'
        raise SystemExit(0)
    a=Client(start('laptop'))
    first=workers()[-1]
    birth_observed=time.monotonic()
    if args.case=='guardian-crash':
        guardian=int(subprocess.run(['ps','-o','ppid=','-p',str(first)],capture_output=True,text=True,check=True).stdout.strip())
        command=subprocess.run(['ps','-o','args=','-p',str(guardian)],capture_output=True,text=True).stdout
        assert '--session-worker' in command,command
        os.kill(guardian,signal.SIGKILL)
        time.sleep(2)
        assert not alive(first), 'guardian SIGKILL leaked server while owner survived'
    elif args.case=='server-crash':
        time.sleep(.5)
        escaped=int((tmp/'detached-child.pid').read_text())
        startup_pids.append(escaped)
        assert alive(escaped)
        os.kill(first,signal.SIGKILL)
        time.sleep(2)
        print(json.dumps({'serverKilled':first,'escapedChild':escaped,'escapedStillAlive':alive(escaped)}),flush=True)
        assert not alive(escaped), 'server crash leaked previously live setsid descendant'
    elif args.case=='idle-empty':
        time.sleep(5)
        assert alive(first), 'empty idle connection was disconnected'
        assert any(e['event']=='idle_reclaim_noop' for e in events()), events()
        assert a.call('thread/loaded/list',{})['data']==[]
    elif args.case=='hard-fd':
        time.sleep(5)
        assert not alive(first), 'hard FD stop did not retire worker'
        assert any(e['event']=='fd_hard_stop' for e in events()), events()
    elif args.case=='contention':
        second=start('laptop')
        time.sleep(1)
        assert second.poll() is not None and second.returncode != 0, 'second live connection was NOT rejected'
        assert workers()==[first], 'contention allocated another worker'
        assert a.call('thread/loaded/list',{})['data']==[]
    elif args.case in ['isolation','stop-isolation']:
        b=Client(start('desktop'))
        assert len(set(workers()))==2, workers()
        if args.case=='stop-isolation':
            stop_env=env.copy(); stop_env.pop('SSH_ORIGINAL_COMMAND')
            stopped=subprocess.run([args.entry,'--stop-client','laptop'],env=stop_env,capture_output=True,text=True,timeout=10)
            assert stopped.returncode==0, stopped.stderr
            time.sleep(2)
            assert not alive(first), 'targeted stop did not retire laptop'
            assert alive(workers()[-1]), 'targeted stop also killed desktop'
        else: a.close()
        assert b.call('thread/loaded/list',{})['data']==[]
        b.close()
    else:
        baseline=fd_counts(first) if args.case in ['idle-thread','idle-thread-stubborn'] else None
        thread=a.call('thread/start',{'cwd':str(tmp),'approvalPolicy':'on-request' if args.case=='approval' else 'never','sandbox':'read-only'})['thread']['id']
        if args.case in ['idle-thread','idle-thread-stubborn']:
            deadline=time.monotonic()+15
            while time.monotonic()<deadline and not mcp_log.exists(): time.sleep(.1)
            while time.monotonic()<deadline:
                mcp_events=[json.loads(line) for line in mcp_log.read_text().splitlines()] if mcp_log.exists() else []
                if any(event['event']=='started' for event in mcp_events): break
                time.sleep(.1)
            else: raise AssertionError('MCP probe did not start')
            mcp_pid=next(event['pid'] for event in mcp_events if event['event']=='started')
            active_resources=fd_counts(first)
            deadline=time.monotonic()+15
            while time.monotonic()<deadline and not any(e['event']=='idle_unsubscribe_completed' for e in events()):
                time.sleep(.2)
            assert alive(first), 'idle unsubscribe disconnected the worker'
            assert any(e['event']=='idle_unsubscribe_completed' for e in events()), events()
            assert a.call('thread/loaded/list',{})['data']==[]
            assert workers()==[first], 'idle unsubscribe replaced the worker'
            settled_resources=fd_counts(first)
            assert not alive(mcp_pid), 'MCP child survived thread unload'
            assert active_resources['fd']>settled_resources['fd'], (baseline,active_resources,settled_resources)
            assert active_resources['pipe']>settled_resources['pipe'], (baseline,active_resources,settled_resources)
            assert (real/'config.toml').read_bytes()==real_config, 'original config mutated'
            print(json.dumps({'case':args.case,'passed':True,'workers':workers(),'mcpPid':mcp_pid,'baseline':baseline,'active':active_resources,'settled':settled_resources,'mockRequests':requests,'temp':str(tmp)}),flush=True)
            raise SystemExit(0)
        turn_params={'threadId':thread,'input':[{'type':'text','text':'isolated no-op acceptance'}]}
        if args.case=='user-input': turn_params['collaborationMode']={'mode':'plan','settings':{'model':'probe'}}
        turn=a.call('turn/start',turn_params)['turn']['id']
        assert received.wait(15), 'mock request missing'
        capture_resources()
        if args.case in ['approval','user-input']:
            request_method='item/commandExecution/requestApproval' if args.case=='approval' else 'item/tool/requestUserInput'
            while True:
                msg=a.other.pop(0) if a.other else a.read()
                if msg.get('method')==request_method: break
            original_approval=msg
            time.sleep(4)
            assert alive(first), 'waiting approval was reclaimed'
            status=a.call('thread/read',{'threadId':thread,'includeTurns':False})['thread']['status']
            flag='waitingOnApproval' if args.case=='approval' else 'waitingOnUserInput'
            assert status=={'type':'active','activeFlags':[flag]},status
        if args.case=='idle-active':
            time.sleep(5)
            assert alive(first), 'active turn was reclaimed by idle threshold'
            current=a.call('thread/read',{'threadId':thread,'includeTurns':True})
            assert any(t['id']==turn and t['status']=='inProgress' for t in current['thread']['turns'])
            assert not any(e['event']=='idle_reclaim' for e in events()), events()
        a.close()
        if args.case in ['approval','user-input']:
            time.sleep(.5)
            b=Client(start('laptop'))
            b.call('thread/resume',{'threadId':thread})
            while True:
                msg=b.other.pop(0) if b.other else b.read()
                if msg.get('method')==request_method: break
            assert msg['id']==original_approval['id']
            assert msg['params']['turnId']==turn
            answer={'decision':'cancel'}
            if args.case=='user-input':
                release.set()
                answer={'answers':{'choice':{'answers':['Skip']}}}
            b.send({'id':msg['id'],'result':answer})
            time.sleep(.3)
            cancelled=b.call('thread/read',{'threadId':thread,'includeTurns':True})['thread']
            assert cancelled['status']['type']=='idle', cancelled['status']
            expected_status='interrupted' if args.case=='approval' else 'completed'
            assert any(t['id']==turn and t['status']==expected_status for t in cancelled['turns']),cancelled
            assert requests==(1 if args.case=='approval' else 2) and workers()==[first]
            print(json.dumps({'interactionReplayed':request_method,'explicitResponse':answer,'sameTurn':turn}),flush=True)
            b.close()
        elif args.case in ['reconnect','offline']:
            if args.case=='offline': release.set()
            time.sleep(.5)
            b=Client(start('laptop'))
            result=b.call('thread/resume',{'threadId':thread})
            assert workers()==[first], 'reconnection spawned another worker'
            expected='completed' if args.case=='offline' else 'inProgress'
            assert any(t['id']==turn and t['status']==expected for t in result['thread']['turns']), result
            assert requests==1, 'model request duplicated'
            release.set()
            deadline=time.monotonic()+15
            while args.case!='offline' and time.monotonic()<deadline:
                v=b.read()
                if v.get('method')=='turn/completed':
                    assert v['params']['turn']['id']==turn
                    break
            else:
                if args.case!='offline': raise AssertionError('same turn did not complete')
            b.close()
        elif args.case=='expiry':
            time.sleep(6)
            assert not alive(first), 'worker survived expired detach lease'
            b=Client(start('laptop'))
            assert workers()[-1]!=first, 'expired worker reused'
            b.close()
        elif args.case=='lifetime':
            while time.monotonic()-birth_observed<9 and alive(first):
                time.sleep(.5)
                try:
                    b=Client(start('laptop')); b.close()
                    if alive(first): assert workers()==[first], 'new worker coexists before old lifetime ends'
                except (EOFError,BrokenPipeError): pass
            assert not alive(first), 'reconnect reset absolute lifetime'
            assert time.monotonic()-birth_observed<9, 'absolute lifetime deadline exceeded'
        elif args.case=='crash':
            owner=owner_ancestor(first)
            assert owner>1
            os.kill(owner,signal.SIGKILL)
            time.sleep(2)
            assert not alive(first), 'reaper left worker alive after owner SIGKILL'
            b=Client(start('laptop'))
            assert workers()[-1]!=first, 'crashed owner did not permit safe replacement'
            b.close()
        elif args.case=='budget':
            for _ in range(4):
                time.sleep(.8)
                if not alive(first): break
                try:
                    b=Client(start('laptop')); b.close()
                except (EOFError,BrokenPipeError): pass
            assert not alive(first), 'reattachment reset cumulative detach budget'
    assert (real/'config.toml').read_bytes()==real_config, 'original config mutated'
    print(json.dumps({'case':args.case,'passed':True,'workers':workers(),'mockRequests':requests,'temp':str(tmp)}),flush=True)
finally:
    print(json.dumps({'evidenceRoot':str(tmp)}),flush=True)
    release.set()
    for p in children:
        try:
            if not p.stdin.closed: p.stdin.close()
            if not p.stdout.closed: p.stdout.close()
        except OSError: pass
    time.sleep(10)
    surviving=[pid for pid in workers() if alive(pid)]
    startup_surviving=[pid for pid in startup_pids if alive(pid)]
    owned_surviving=[pid for pid in owned_pids if alive(pid)]
    for pid in surviving+startup_surviving:
        try: os.killpg(pid,signal.SIGKILL)
        except ProcessLookupError: pass
    for p in children:
        if p.poll() is None:
            try: os.killpg(p.pid,signal.SIGKILL)
            except ProcessLookupError: pass
        p.wait(timeout=5)
    print(json.dumps({'cleanupIntervention':surviving+startup_surviving,'remainingWorkers':[pid for pid in workers()+startup_pids if alive(pid)],'remainingOwnedProcesses':[pid for pid in owned_pids if alive(pid)]}),flush=True)
    assert not surviving and not startup_surviving and not owned_surviving, 'acceptance required forced cleanup or left owned processes'
