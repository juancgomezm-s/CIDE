"""Build once in a reusable Cargo cache, export immutable local deliveries."""
import argparse,hashlib,json,os,shutil,subprocess
from pathlib import Path

def main():
    parser=argparse.ArgumentParser(description=__doc__)
    parser.add_argument('--output',required=True,type=Path,help='New delivery directory inside workspace/target')
    parser.add_argument('--jobs',type=int,default=2,choices=range(1,5))
    args=parser.parse_args()
    root=Path(__file__).resolve().parents[1]
    target=(root/'target').resolve(strict=True)
    output=args.output if args.output.is_absolute() else root/args.output
    output=output.resolve(strict=False)
    cache=target/'build-cache'
    if not output.is_relative_to(target) or output==target or output.is_relative_to(cache) or output.exists():
        parser.error('Use a new delivery directory inside target, outside build-cache')
    env=os.environ.copy();env['CARGO_TARGET_DIR']=str(cache);env['CARGO_INCREMENTAL']='0'
    toolchain=root/'work/toolchain'
    local=toolchain/'cargo/bin/cargo.exe'
    cargo=str(local) if local.is_file() else shutil.which('cargo')
    if not cargo:parser.error('No existing Cargo toolchain found; nothing installed')
    if local.is_file():
        env.update(CARGO_HOME=str(toolchain/'cargo'),RUSTUP_HOME=str(toolchain/'rustup'))
    temp=root/'work/test-temp';temp.mkdir(parents=True,exist_ok=True)
    env.update(TEMP=str(temp),TMP=str(temp))
    subprocess.run([cargo,'build','--workspace','--release','--locked','--offline','-j',str(args.jobs)],cwd=root,env=env,check=True)
    names=[name+'.exe' for name in ['cide','cide-tui','cide-gui','jc-ide','jc-ide-tui','jc-ide-gui']]
    source=cache/'release'
    if not all((source/name).is_file()for name in names):raise RuntimeError('Incomplete build; no delivery exported')
    output.mkdir(parents=True,exist_ok=False)
    files={}
    for path in sorted(source.iterdir()):
        if path.is_file()and path.suffix.lower()in {'.exe','.pdb','.dll'}:
            destination=output/path.name;shutil.copyfile(path,destination)
            digest=hashlib.sha256(path.read_bytes()).hexdigest()
            if hashlib.sha256(destination.read_bytes()).hexdigest()!=digest:raise RuntimeError('Delivery checksum mismatch')
            files[path.name]=digest
    (output/'build-manifest.json').write_text(json.dumps({'cache':str(cache),'files':files},indent=2)+'\n',encoding='utf-8')
    print(json.dumps({'ok':True,'delivery':str(output),'files':len(files),'cache':str(cache)}))

if __name__=='__main__':main()
