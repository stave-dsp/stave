#!/usr/bin/env python3
"""Exercise the real shell function through a Windows-like extra shell layer."""
import os
from pathlib import Path
import subprocess
import tempfile
import unittest

SCRIPT = Path(__file__).resolve().parents[1] / 'offload.sh'
SOURCE = SCRIPT.read_text()
FUNCTION = SOURCE[SOURCE.index('runner() {'):SOURCE.index('\n# Run a shell command on HOST')]


class TransportTests(unittest.TestCase):
    def launch(self, code, stdin='payload $HOME\n', status=0):
        with tempfile.TemporaryDirectory() as directory:
            root = Path(directory)
            runner = root / 'source.pl'
            # Larger than the Windows command-line limit after base64 encoding.
            runner.write_text('# padding\n' * 4000 + code)
            ssh = root / 'ssh'
            ssh.write_text('''#!/usr/bin/env python3
import os, subprocess, sys
command=sys.argv[2]
if len(command)>32767: sys.exit(255)
# The command itself goes through an outer double-quoted shell argument.
os.chdir(os.environ['REMOTE_HOME'])
sys.exit(subprocess.run(['bash','-c','bash -c "'+command+'"']).returncode)
''')
            ssh.chmod(0o755)
            env = dict(os.environ, PATH=str(root) + ':' + os.environ['PATH'], REMOTE_HOME=str(root), HOME=str(root), LC_ALL='C')
            result = subprocess.run(['bash', '-c', FUNCTION + '\nrunner fake probe "argument with spaces"', '_'],
                input=stdin, text=True, capture_output=True,
                env=dict(env, RUNNER=str(runner), RUNNER_ID='a' * 64))
            self.assertEqual(result.returncode, status, result.stderr)
            return result

    def test_large_runner_and_stdin(self):
        result = self.launch('print while <STDIN>; exit 0;')
        self.assertEqual(result.stdout, 'payload $HOME\n')

    def test_runner_exit_status(self):
        self.launch('exit 23;', status=23)

    def test_upload_compile_failure(self):
        result = self.launch('this is not valid Perl !!!', status=255)
        self.assertIn('syntax error', result.stderr)

    def test_runtime_failure(self):
        result = self.launch('die "runtime sentinel\\n";', status=255)
        self.assertIn('runtime sentinel', result.stderr)


if __name__ == '__main__':
    unittest.main()
