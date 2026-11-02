import ctypes as c,json,os,platform,winreg
from ctypes import wintypes as w
from pathlib import Path
from datetime import datetime,timezone
k=c.WinDLL('kernel32',use_last_error=True)
k.GetCurrentProcess.restype=w.HANDLE
k.GetActiveProcessorCount.argtypes=[w.WORD];k.GetActiveProcessorCount.restype=w.DWORD
process_mask=c.c_size_t();system_mask=c.c_size_t()
k.GetProcessAffinityMask.argtypes=[w.HANDLE,c.POINTER(c.c_size_t),c.POINTER(c.c_size_t)]
if not k.GetProcessAffinityMask(k.GetCurrentProcess(),c.byref(process_mask),c.byref(system_mask)):raise c.WinError(c.get_last_error())
length=w.DWORD()
k.GetLogicalProcessorInformationEx.argtypes=[c.c_int,c.c_void_p,c.POINTER(w.DWORD)]
k.GetLogicalProcessorInformationEx(0,None,c.byref(length))
buffer=c.create_string_buffer(length.value)
if not k.GetLogicalProcessorInformationEx(0,buffer,c.byref(length)):raise c.WinError(c.get_last_error())
offset=cores=0
while offset<length.value:
 relation,size=c.cast(c.byref(buffer,offset),c.POINTER(w.DWORD*2)).contents
 assert size>=8 and offset+size<=length.value
 if relation==0:cores+=1
 offset+=size
with winreg.OpenKey(winreg.HKEY_LOCAL_MACHINE,r'HARDWARE\DESCRIPTION\System\CentralProcessor\0') as key:
 name=winreg.QueryValueEx(key,'ProcessorNameString')[0].strip()
data={'utc':datetime.now(timezone.utc).isoformat(),'os':platform.platform(),'processor':name,'physical_cores':cores,'active_logical_processors_all_groups':k.GetActiveProcessorCount(0xffff),'python_os_cpu_count':os.cpu_count(),'current_process_affinity_mask':hex(process_mask.value),'current_process_available_logical_bits':process_mask.value.bit_count(),'system_affinity_mask':hex(system_mask.value),'affinity_changed':False,'concurrent_qwen_load':'unknown; no other process inspection or control','scope':'read-only OS processor topology/own process affinity; not a CPU utilization benchmark'}
path=Path('docs/benchmarks/M2-cpu-hardware.json')
if path.exists():raise FileExistsError(path)
path.write_text(json.dumps(data,ensure_ascii=False,indent=2)+'\n',encoding='utf-8',newline='\n')
print(json.dumps(data,ensure_ascii=False,indent=2))
