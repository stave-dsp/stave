import os
from pathlib import Path
import subprocess
import tempfile
import unittest

ROOT = Path(__file__).resolve().parents[2]


class Toolchain(unittest.TestCase):
    def test_fleet_verifies_and_hosted_installs(self):
        with tempfile.TemporaryDirectory() as directory:
            path = Path(directory)
            tool = path / 'rustup'
            tool.write_text('#!/bin/sh\nprintf "%s\\n" "$*" > "$TRACE"\nexit "${RESULT:-0}"\n')
            tool.chmod(0o755)
            trace = path / 'trace'
            env = dict(os.environ, PATH=directory+':'+os.environ['PATH'], TRACE=str(trace))
            for mode, expected in [('self-hosted', 'run 1.86.0 rustc --version'), ('github-hosted', 'toolchain install 1.86.0 --profile minimal')]:
                env['RUNNER_ENVIRONMENT'] = mode
                subprocess.run([str(ROOT/'scripts/ci-toolchain.sh'),'1.86.0'], env=env, check=True)
                self.assertEqual(trace.read_text().strip(), expected)
            env.update(RUNNER_ENVIRONMENT='self-hosted', RESULT='1')
            failed = subprocess.run([str(ROOT/'scripts/ci-toolchain.sh'),'1.86.0'], env=env)
            self.assertNotEqual(failed.returncode, 0)
