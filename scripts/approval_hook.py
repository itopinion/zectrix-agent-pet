#!/usr/bin/env python3
"""Observe Codex hooks. Always exit 0 silently; NEVER emit an approval decision.

Only identifiers, title, tool name and time go to the local pet app. Command text,
tool arguments, results, credentials and approval decisions are never forwarded.
"""
import fcntl
import hashlib
import json
import os
from pathlib import Path
import sys
import time
import urllib.parse
import urllib.request
import uuid

ALLOWED = {'PermissionRequest', 'PostToolUse', 'Stop', 'Interrupt', 'SessionEnd'}

def main():
    connection = Path(sys.argv[1]).resolve()
    payload = json.loads(sys.stdin.buffer.read(2 * 1024 * 1024))
    event = payload.get('hook_event_name')
    thread = payload.get('session_id', '')
    turn = payload.get('turn_id', '')
    if event not in ALLOWED or not thread:
        return
    if event in ('PermissionRequest', 'PostToolUse') and not turn:
        return
    tool = payload.get('tool_name', '')
    raw = payload.get('tool_input', {})
    # Descriptions can be attached only to the approval prompt, not tool output.
    raw = {k: v for k, v in raw.items() if k != 'description'} if isinstance(raw, dict) else raw
    fingerprint = hashlib.sha256(json.dumps([thread, turn, tool, raw], sort_keys=True, ensure_ascii=False).encode()).hexdigest()
    os.umask(0o077)
    lock = connection.parent / 'approval-hook.lock'
    cache_file = connection.parent / 'approval-hook-state.json'
    with lock.open('a') as handle:
        fcntl.flock(handle, fcntl.LOCK_EX)
        cache = json.loads(cache_file.read_text()) if cache_file.exists() else {}
        cache = {k: v for k, v in cache.items() if time.time() - v['at'] < 86400}
        if event == 'PermissionRequest':
            cache.setdefault(fingerprint, {'id': uuid.uuid4().hex, 'thread': thread, 'turn': turn, 'at': time.time()})
            request_id = cache[fingerprint]['id']
        elif event == 'PostToolUse':
            item = cache.pop(fingerprint, None)
            if item is None:
                return
            request_id = item['id']
        else:
            affected = [k for k, v in cache.items() if v['thread'] == thread and (not turn or v['turn'] == turn)]
            if not affected:
                return
            for k in affected:
                del cache[k]
            request_id = ''
        title = 'Codex 权限审批'
        home = Path(os.environ.get('CODEX_HOME', str(Path.home() / '.codex')))
        index = home / 'session_index.jsonl'
        if index.exists():
            with index.open('rb') as f:
                f.seek(max(0, index.stat().st_size - 4 * 1024 * 1024))
                for line in f.read().splitlines():
                    try:
                        item = json.loads(line)
                        if item.get('id') == thread:
                            title = item.get('thread_name') or title
                    except (ValueError, UnicodeError):
                        pass
        signal = {'event': event, 'thread_id': thread, 'turn_id': turn, 'request_id': request_id,
                  'tool_name': tool, 'title': title, 'at': int(time.time())}
        conn = json.loads(connection.read_text())
        url = urllib.parse.urlsplit(conn['url'])
        if url.scheme != 'http' or url.hostname not in ('127.0.0.1', 'localhost') or url.username or url.password or url.query or url.fragment or url.path not in ('', '/'):
            return
        class NoRedirect(urllib.request.HTTPRedirectHandler):
            def redirect_request(self, *args, **kwargs):
                return None
        opener = urllib.request.build_opener(urllib.request.ProxyHandler({}), NoRedirect())
        req = urllib.request.Request(conn['url'].rstrip('/') + '/api/approvals',
            data=json.dumps(signal).encode(), headers={'Authorization': 'Bearer ' + conn['token'], 'Content-Type': 'application/json'})
        with opener.open(req, timeout=1) as response:
            response.read(4096)
        temporary = cache_file.with_suffix('.tmp')
        temporary.write_text(json.dumps(cache))
        os.replace(temporary, cache_file)

if __name__ == '__main__':
    try:
        main()
    except Exception:
        # A disconnected pet must never block or decide a Codex approval.
        pass
