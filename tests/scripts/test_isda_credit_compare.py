"""Failure controls using a frozen native control replay, never a live-engine claim.
CI separately executes the current compiled native binary against the original.
"""
import copy,json,subprocess,sys,tempfile,unittest
from pathlib import Path
import isda_credit_compare as subject
class CreditComparisonControls(unittest.TestCase):
 @classmethod
 def setUpClass(cls):
  cls.data,cls.original,cls.mixed=subject.reference();cls.control=subject.loads((subject.ROOT/subject.PACK/'native-control.json').read_bytes())
 def check(self,value):return subject.validate_candidate(value,self.data,self.original,self.mixed)
 def reject(self,value):
  with self.assertRaises((ValueError,KeyError,TypeError)):self.check(value)
 def test_control_replay_only_positive_protocol(self):
  result=self.check(self.control);self.assertEqual(result['status'],'passed');self.assertEqual(result['compared_values'],1150)
 def test_duplicate_nonfinite_and_size_rejection(self):
  for raw in [b'{"a":1,"a":2}',b'{"a":NaN}',b'{"a":Infinity}',b' '*((8*1024*1024)+1)]:
   with self.assertRaises(ValueError):subject.loads(raw)
 def test_original_scope_order_and_notice(self):
  for mutate in [lambda c:c['experiments'].pop(),lambda c:c['experiments'].reverse(),lambda c:c.update(notice='wrong'),lambda c:c['sensitivities'].pop()]:
   c=copy.deepcopy(self.control);mutate(c);self.reject(c)
 def test_native_quote_units_dates_and_entity(self):
  for path,value in [('unit','DiscountFactor'),('date','2022-06-25'),('entity','Other.Entity'),('coupon','0.03')]:
   c=copy.deepcopy(self.control);h=c['experiments'][0]['calibration']['recipe']['helpers'][0]
   if path=='unit':h['quote']['unit']=value
   elif path=='date':h['context']['step_in_date']=value
   elif path=='entity':h['terms']['reference']['entity_id']=value
   else:h['terms']['coupon_decimal']=value
   self.reject(c)
 def test_solver_residual_rank_and_held_calibration(self):
  for mutate in [lambda c:c['experiments'][0]['calibration']['repricing'][0].update(residual=1.0),lambda c:c['additional_fits'][0]['fit']['global'].update(rank=5),lambda c:c['experiments'][1].update(calibration=c['experiments'][0]['calibration'])]:
   c=copy.deepcopy(self.control);mutate(c);self.reject(c)
 def test_missing_risk_to_zero_is_mismatch(self):
  c=copy.deepcopy(self.control);c['sensitivities'][0]['value']=0;self.assertEqual(self.check(c)['status'],'numerical_mismatch')
 def test_numerical_survival_and_currency_mismatches(self):
  for mutate in [lambda c:c['experiments'][0]['survival_probabilities'].__setitem__(0,1.1),lambda c:c['experiments'][0]['valuations'][0].update(net_pv=1e9)]:
   c=copy.deepcopy(self.control);mutate(c)
   result=self.check(c);self.assertEqual(result['status'],'numerical_mismatch');self.assertTrue(result['errors'])
 def test_risk_units_cannot_be_relabelled(self):
  c=copy.deepcopy(self.control);c['sensitivities'][0]['measure']='Cs01Signed';self.reject(c)
 def test_original_pin_and_bounds_are_not_refreshable(self):
  with tempfile.TemporaryDirectory() as tmp:
   root=Path(tmp)
   paths=set(subject.PINS)|{(subject.base.PACK/p).as_posix() for p in subject.base.PINS}|{subject.base.DRIVER,subject.base.LICENSE}
   for path in paths:
    p=root/path;p.parent.mkdir(parents=True,exist_ok=True);p.write_bytes((subject.ROOT/path).read_bytes())
   p=root/subject.PACK/'inputs.json';data=subject.loads(p.read_bytes());data['bounds']['pv_currency_absolute']=1000000;p.write_text(json.dumps(data))
   with self.assertRaises(ValueError):subject.reference(root)
 def test_guards_remain_enabled_with_python_optimized(self):
  code="import isda_credit_compare as c;c.loads(b'{\"a\":1,\"a\":2}')"
  p=subprocess.run([sys.executable,'-B','-O','-c',code],cwd=Path(__file__).resolve().parent,capture_output=True,text=True)
  self.assertNotEqual(p.returncode,0);self.assertIn('duplicate JSON key',p.stderr)
if __name__=='__main__':unittest.main()
