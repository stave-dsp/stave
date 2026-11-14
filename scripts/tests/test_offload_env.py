import subprocess
import tempfile
import unittest
from pathlib import Path
SCRIPT = Path(__file__).resolve().parents[1] / 'lib/offload-env.pl'
class Environments(unittest.TestCase):
    def run_manifest(self, text, host='worker'):
        with tempfile.TemporaryDirectory() as d:
            p=Path(d)/'env.tsv';p.write_text(text)
            return subprocess.run(['perl',str(SCRIPT),str(p),host],text=True,capture_output=True)
    def test_declared_layout(self):
        result=self.run_manifest('worker\t.fleet/env\n')
        self.assertEqual(result.returncode,0)
        self.assertEqual(result.stdout,'. "$HOME/.fleet/env"')
    def test_invalid_unknown_duplicate_and_shell_payload(self):
        for text in ['other\t.cargo/env\n','worker\t../secret\n','worker\tx;touch /tmp/bad\n','worker\t.cargo/env\nworker\t.fleet/env\n','worker\t/absolute/env\n','worker\t$(id)\n','worker .fleet/env\n']:
            with self.subTest(text=text):
                result=self.run_manifest(text)
                self.assertNotEqual(result.returncode,0)
                self.assertEqual(result.stdout,'')
if __name__=='__main__':unittest.main()
