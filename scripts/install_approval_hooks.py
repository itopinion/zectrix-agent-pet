#!/usr/bin/env python3
"""Install observational hooks. Leaves Codex trust and approval policy untouched."""
import argparse
import datetime
import json
from pathlib import Path
import shlex
import shutil
import sys

parser=argparse.ArgumentParser()
parser.add_argument('--codex-home',type=Path,required=True)
parser.add_argument('--connection',type=Path,required=True)
args=parser.parse_args()
connection=args.connection.resolve();home=args.codex_home.resolve()
assert connection.is_file(), 'Start the pet app first'
target=connection.parent/'approval_hook.py'
shutil.copy2(Path(__file__).with_name('approval_hook.py'),target)
config=home/'hooks.json'
data=json.loads(config.read_text()) if config.exists() else {}
if config.exists():
    backup=config.with_name('hooks.before-agent-pet-'+datetime.datetime.now().strftime('%Y%m%d-%H%M%S')+'.json')
    shutil.copy2(config,backup)
command=' '.join(shlex.quote(x) for x in [sys.executable,str(target),str(connection)])
for event in ['PermissionRequest','PostToolUse','Stop','Interrupt','SessionEnd']:
    groups=data.setdefault('hooks',{}).setdefault(event,[])
    # Keep all unrelated hooks. Only replace our own named handlers.
    for group in groups:
        group['hooks']=[h for h in group.get('hooks',[]) if h.get('statusMessage')!='ZECTRIX 宠物审批提醒']
    groups[:]=[group for group in groups if group.get('hooks')]
    groups.append({'hooks':[{'type':'command','command':command,'timeout':2,'statusMessage':'ZECTRIX 宠物审批提醒'}]})
temporary=config.with_suffix('.tmp');temporary.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n');temporary.replace(config)
print('Hook definitions installed. Review and trust them using Codex /hooks. Approval policy and hook trust were not changed.')
