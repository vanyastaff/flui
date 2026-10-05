from pathlib import Path
import os, re, subprocess, json, time, uuid
root=Path.cwd(); originals={}
backup_dir=Path.home()/'.codex/research/engine-filter-audit'/('measurement-backup-'+uuid.uuid4().hex)
backup_dir.mkdir(parents=True)
print('Source backups: '+str(backup_dir),flush=True)
def restore_exact(path, data):
 for attempt in range(20):
  try:
   path.write_bytes(data); assert path.read_bytes()==data; return
  except OSError:
   if attempt==19: raise
   time.sleep(.2)
files=['texture_pool.rs','layer_offscreen.rs','blur/mod.rs','morphology/mod.rs','replay/flush.rs','replay/ordered.rs','replay/coverage.rs','clip_mask.rs','replay/composite.rs','advanced_blend/mod.rs','ssaa.rs','blur_filter_tests/foreground.rs']
try:
 for relative in files:
  p=root/'crates/flui-engine/src'/relative; originals[p]=p.read_bytes()
  saved=backup_dir/relative; saved.parent.mkdir(parents=True,exist_ok=True); saved.write_bytes(originals[p])
  s=originals[p].decode('utf-8')
  s=re.sub(r'(?m)^(\s*)(let (?:mut )?\w+ = )encoder\.begin_render_pass', r'\1eprintln!("MEASURE_PASS");\n\1\2encoder.begin_render_pass',s)
  if relative=='texture_pool.rs':
   old='self.inventory.total_memory_bytes += desc.size_bytes();';assert old in s
   s=s.replace(old,old+'\n            eprintln!("MEASURE_POOL bytes={} width={} height={}", self.inventory.total_memory_bytes, width, height);')
  if relative=='blur_filter_tests/foreground.rs':
   s+='''
#[test]
fn foreground_filter_measurement_only() {
 let (device, queue)=crate::test_support::test_device_and_queue("Foreground Measurement Device");
 let selected=cases().into_iter().filter(|c| matches!(c.name,"left"|"huge_source"|"distant_large_coordinates"|"double_blur"|"nested_filter"));
 for c in selected {
  let mut painter=WgpuPainter::with_shared_device(Arc::clone(&device),Arc::clone(&queue),wgpu::TextureFormat::Rgba8Unorm,(32,32));
  for sample in 0..13 {
   eprintln!("MEASURE_START name={} sample={}",c.name,sample);
   let start=std::time::Instant::now();
   let pixels=render_direct(&mut painter,c,0.0,32,true);
   eprintln!("MEASURE_END elapsed_ms={:.6} visible={}",start.elapsed().as_secs_f64()*1000.0,pixels.chunks_exact(4).any(|p|p[0]<254));
  }
 }
}
'''
  p.write_bytes(s.encode('utf-8'))
 env=os.environ.copy();env['CARGO_BUILD_JOBS']='1';env['FLUI_REQUIRE_GPU']='1'
 log=root/'docs/research/foreground-filter-measurement.log'
 with log.open('wb') as f:
  r=subprocess.run(['cargo','test','-p','flui-engine','--features','testing','--lib','foreground_filter_measurement_only','--locked','--','--nocapture','--test-threads=1'],env=env,stdout=f,stderr=subprocess.STDOUT)
 assert r.returncode==0,log.read_text(encoding='utf-8',errors='replace')[-4000:]
 rows=[]; current=None
 for line in log.read_text(encoding='utf-8').splitlines():
  if 'MEASURE_START' in line:
   m=re.search(r'name=(\S+) sample=(\d+)',line);current={'name':m[1],'sample':int(m[2]),'passes':0,'pool_allocation_high_water_bytes':0,'allocations':[]}
  elif 'MEASURE_PASS' in line and current is not None: current['passes']+=1
  elif 'MEASURE_POOL' in line and current is not None:
   m=re.search(r'bytes=(\d+) width=(\d+) height=(\d+)',line);current['pool_allocation_high_water_bytes']=max(current['pool_allocation_high_water_bytes'],int(m[1]));current['allocations'].append([int(m[2]),int(m[3])])
  elif 'MEASURE_END' in line and current is not None:
   current['elapsed_ms']=float(re.search(r'elapsed_ms=([\d.]+)',line)[1]);current['visible']='visible=true' in line;rows.append(current);current=None
 (root/'docs/research/foreground-filter-measurements.json').write_text(json.dumps(rows,indent=2)+'\n',encoding='utf-8')
 print('Measurement PASS; restoring sources',flush=True)
finally:
 failures=[]
 for p,b in originals.items():
  try: restore_exact(p,b)
  except OSError as error: failures.append((str(p),str(error)))
 if failures: raise RuntimeError('Restoration failed; originals remain in '+str(backup_dir)+': '+str(failures))
 print('All instrumented sources restored byte for byte',flush=True)
