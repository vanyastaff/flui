from pathlib import Path
import os, subprocess, time, uuid
root = Path.cwd()
backup_dir=Path.home()/'.codex/research/engine-filter-audit'/('counterfactual-backup-'+uuid.uuid4().hex)
backup_dir.mkdir(parents=True)
print('Source backups: '+str(backup_dir),flush=True)
def restore_exact(path, data):
 for attempt in range(20):
  try:
   path.write_bytes(data); assert path.read_bytes()==data; return
  except OSError:
   if attempt==19: raise
   time.sleep(.2)
cases = [
 ('source-viewport-intersection', 'crates/flui-engine/src/painter/layer.rs', '            if input_support.is_empty() && !creates_alpha {', '            let input_support = input_support.intersect(&self.viewport_bounds()).unwrap_or(Rect::ZERO);\n            if input_support.is_empty() && !creates_alpha {', 'foreground_filter_viewport_crop_contract'),
 ('original-support-reused', 'crates/flui-engine/src/layer_offscreen.rs', '                    support,', '                    input_support,', 'foreground_filter_chains_match_independent_nested_layers'),
 ('mixed-glyph-source-omitted', 'crates/flui-engine/src/painter/layer.rs', '|| !segment.glyph_batch.is_empty()', '|| false', 'foreground_filter_viewport_crop_contract'),
 ('shader-circle-radius-omitted', 'crates/flui-engine/src/painter/layer.rs', 'instance.center_radius[2].max(1e-6)', 'instance.center_radius[2]', 'foreground_filter_viewport_crop_contract'),
]
env = os.environ.copy(); env['CARGO_BUILD_JOBS']='1'; env['FLUI_REQUIRE_GPU']='1'
for slug, relative, old, new, name in cases:
 path=root/relative; original=path.read_bytes(); (backup_dir/(slug+'.rs')).write_bytes(original)
 source=original.decode('utf-8'); assert old in source
 try:
  path.write_bytes(source.replace(old,new).encode('utf-8'))
  log=root/'docs/research'/('foreground-filter-counterfactual-'+slug+'.log')
  with log.open('wb') as out:
   result=subprocess.run(['cargo','test','-p','flui-engine','--features','testing','--lib','blur_filter_tests::foreground::'+name,'--locked','--','--exact','--nocapture','--test-threads=1'], env=env, stdout=out, stderr=subprocess.STDOUT)
  body=log.read_text(encoding='utf-8',errors='replace')
  assert result.returncode==101 and 'test result: FAILED' in body, (slug,result.returncode,body[-2000:])
  print(slug+': intended test FAILED; exit 101',flush=True)
 finally:
  restore_exact(path,original)
