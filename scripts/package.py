#!/usr/bin/env python3
"""Build a Hub v1 ZIP from an explicitly selected native executable."""
import argparse, hashlib, json, platform, stat, zipfile
from pathlib import Path

def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--binary', type=Path, required=True)
    parser.add_argument('--out', type=Path, default=Path('dist'))
    args=parser.parse_args()
    root=Path(__file__).resolve().parents[1]
    if platform.system()!='Darwin' or platform.machine()!='arm64':
        raise SystemExit('This release target requires Apple Silicon macOS; do not relabel a foreign executable.')
    binary=args.binary.read_bytes()
    # Mach-O 64-bit little endian + ARM64 CPU type.
    if binary[:8]!=bytes.fromhex('cffaedfe0c000001'):
        raise SystemExit('Expected a native ARM64 Mach-O executable')
    manifest=json.loads((root/'app.json').read_text())
    files={manifest['entry']:binary}
    for name in ['README.md','docs/HUB_CONTRACT.md','LICENSE','AGENT_INTEGRATION.md','THIRD_PARTY_NOTICES.md','APPROVAL_REMINDERS.md','scripts/approval_hook.py','scripts/install_approval_hooks.py']:
        files[name]=(root/name).read_bytes()
    manifest['files']={name:hashlib.sha256(data).hexdigest() for name,data in files.items()}
    args.out.mkdir(parents=True,exist_ok=True)
    path=args.out/f"{manifest['id']}-{manifest['version']}-macos-aarch64.zip"
    with zipfile.ZipFile(path,'w',zipfile.ZIP_DEFLATED) as archive:
        for name,data in {'app.json':(json.dumps(manifest,ensure_ascii=False,indent=2)+'\n').encode(),**files}.items():
            info=zipfile.ZipInfo(name);info.create_system=3;info.compress_type=zipfile.ZIP_DEFLATED
            info.external_attr=(stat.S_IFREG|(0o755 if name==manifest['entry'] else 0o644))<<16
            archive.writestr(info,data)
    with zipfile.ZipFile(path) as archive:
        assert archive.testzip() is None
        assert set(archive.namelist())=={'app.json',*files}
        for name,digest in manifest['files'].items():assert hashlib.sha256(archive.read(name)).hexdigest()==digest
    path.with_suffix('.zip.sha256').write_text(hashlib.sha256(path.read_bytes()).hexdigest()+'  '+path.name+'\n')
    print(path)

if __name__=='__main__':main()
