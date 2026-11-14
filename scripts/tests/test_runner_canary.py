import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]

class RunnerCanary(unittest.TestCase):
    def test_missing_capabilities_fail_without_installing(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            (path / 'sed').symlink_to('/usr/bin/sed')
            for tool in ['rustup', 'cargo', 'ngspice']:
                executable = path / tool
                executable.write_text('#!/bin/bash\nexit 0\n')
                executable.chmod(0o755)
            env = {**os.environ, 'PATH': directory, 'CANARY_LABEL': 'ngspice'}
            def run():
                return subprocess.run(['/bin/bash', str(ROOT / 'scripts/runner-canary.sh')], cwd=ROOT, env=env, capture_output=True).returncode
            self.assertEqual(run(), 0)
            for tool in ['rustup', 'cargo', 'ngspice']:
                with self.subTest(tool=tool):
                    (path / tool).rename(path / (tool + '.off'))
                    self.assertNotEqual(run(), 0)
                    (path / (tool + '.off')).rename(path / tool)
            (path / 'cargo').write_text('#!/bin/bash\n[[ "$1" != nextest ]]\n')
            self.assertNotEqual(run(), 0)

if __name__ == '__main__':
    unittest.main()
