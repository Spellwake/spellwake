"""False-pass guards for the reference evidence collector."""
import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location("evidence",Path(__file__).resolve().parents[2]/"scripts/collect-reference-smoke.py")
assert spec and spec.loader
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)
class EvidenceTests(unittest.TestCase):
    def test_requires_the_exact_named_test(self):
        self.assertTrue(module.case_passed('<testsuite tests="1" failures="0" errors="0" skipped="0"><testcase classname="C" name="m"/></testsuite>',"C","m"))
        self.assertFalse(module.case_passed('<testsuite tests="1" failures="0" errors="0" skipped="0"><testcase classname="C" name="other"/></testsuite>',"C","m"))
    def test_rejects_skipped_failed_and_duplicate_cases(self):
        for child in ['<skipped/>','<failure/>','<error/>']:
            self.assertFalse(module.case_passed(f'<testsuite tests="1" failures="0" errors="0" skipped="0"><testcase classname="C" name="m">{child}</testcase></testsuite>',"C","m"))
        self.assertFalse(module.case_passed('<testsuite tests="2" failures="0" errors="0" skipped="0"><testcase classname="C" name="m"/><testcase classname="C" name="m"/></testsuite>',"C","m"))
    def test_rejects_empty_or_failing_suites(self):
        for count,failures in [("0","0"),("1","1")]:
            self.assertFalse(module.case_passed(f'<testsuite tests="{count}" failures="{failures}" errors="0" skipped="0"><testcase classname="C" name="m"/></testsuite>',"C","m"))


class ExactRunTests(unittest.TestCase):
    def test_requires_exact_named_count_and_no_extra_unknown_cases(self):
        xml = '<testsuite tests="2" failures="0" errors="0" skipped="0"><testcase classname="C" name="a"/><testcase classname="C" name="b"/></testsuite>'
        self.assertTrue(module.run_passed(xml,"C",["a","b"]))
        self.assertFalse(module.run_passed("not XML","C",["a","b"]))
        for methods in [["a"], ["a","c"], ["a","a"], []]:
            self.assertFalse(module.run_passed(xml,"C",methods))
        self.assertFalse(module.run_passed(xml.replace('tests="2"','tests="3"'),"C",["a","b"]))

if __name__ == "__main__": unittest.main()
