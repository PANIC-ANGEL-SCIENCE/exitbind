"""Lexical contracts and synthetic records, not pixel or profile-loader tests."""
import json
from pathlib import Path
import tempfile
import unittest
from check_outcome import check
from test_capture_styling_guidance import framing_mode_fixture
from test_check_run import PNG
ROOT=Path(__file__).resolve().parents[1]
CONTRACTS={
'skills/mockup-image/references/capture-styling.md':[
'A color match and a category name do not preserve a designed silhouette',
'Resolve this brief before generation',
'These are checks, not instructions to add wraps, panels, zippers, pleats or asymmetry',
'Simple straight trousers can be correct for a different brief',
'Compare it with the chosen source relationships and the rejected fallback',
'Structural plausibility is not proof of manufacture'],
'skills/design-pack/references/memory.md':[
'read the actual relevant content before prompting',
'explicit current task, then explicit project direction, then the relevant configured user default',
'source not configured, located but unread, successfully read, missing and access denied',
'do not retry through a bypass',
'a required profile-read check remains not-tested',
'not an automatic loader, background hook or credential/access grant',
'do not claim fresh-session automatic integration without testing the real host flow']}
class GarmentStructureContracts(unittest.TestCase):
 def test_contracts(self):
  for path,cues in CONTRACTS.items():
   text=(ROOT/path).read_text()
   for cue in cues:
    with self.subTest(path=path,cue=cue): self.assertIn(cue,text)
 def test_deletion_guard(self):
  for path,cues in CONTRACTS.items():
   text=(ROOT/path).read_text()
   for cue in cues:
    with self.subTest(cue=cue): self.assertNotIn(cue,text.replace(cue,''))
 def test_brief_routes(self):
  text=(ROOT/'skills/mockup-image/templates/image-brief.md').read_text()
  for cue in ('Garment construction brief','Garment structure review','Private fashion guidance'): self.assertIn(cue,text)
 def test_counterexamples(self):
  text=(ROOT/'EVALUATION.md').read_text()
  for cue in ('Simple straight garment explicitly requested','Profile path present but never read','No configured fashion profile','Saved default conflicts with current task'): self.assertIn(cue,text)
class GarmentStructureDeclaredOutcomes(unittest.TestCase):
 def setUp(self):
  self.temp=tempfile.TemporaryDirectory();self.addCleanup(self.temp.cleanup);self.root=Path(self.temp.name);(self.root/'sample.png').write_bytes(PNG)
  self.data=framing_mode_fixture('synthetic garment task')
  for axis in ('garment-silhouette','garment-construction','private-profile-read','preserved-regions'):
   self.data['requirements'].append({'id':axis,'category':'other','instruction':'Synthetic scoped '+axis,'targets':[{'artifact_id':'sample','location':axis}]})
   self.data['checks'].append({'requirement_id':axis,'artifact_id':'sample','location':axis,'result':'pass','view_ids':['sample-view'],'observation':'Synthetic declared '+axis+'; not actual image or source evidence.'})
 def result(self):
  (self.root/'outcome.json').write_text(json.dumps(self.data));return check(self.root,'outcome.json')
 def test_declared_pass(self): self.assertEqual(self.result(),[])
 def test_failed_structure_blocks(self):
  self.data['checks'][-3]['result']='fail';self.assertIn('unmet scoped requirement blocks readiness',self.result())
 def test_unread_profile_blocks(self):
  self.data['checks'][-2].update(result='not-tested',view_ids=[]);self.assertIn('unmet scoped requirement blocks readiness',self.result())
 def test_missing_required_profile_record_blocks(self):
  self.data['checks'].pop(-2);self.assertTrue(self.result())
 def test_rework_preserves_unread_gap(self):
  self.data['status']='rework';self.data['checks'][-2].update(result='not-tested',view_ids=[]);self.assertEqual(self.result(),[])
 def test_no_profile_not_globally_required(self):
  self.data=framing_mode_fixture('no configured profile');self.assertEqual(self.result(),[])
if __name__=='__main__': unittest.main()
