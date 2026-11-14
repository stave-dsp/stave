"""Regression tests for CI recovery boundaries and ruleset preservation."""
import copy
import importlib.util
from pathlib import Path
import unittest
from unittest.mock import patch
import os
import tempfile
import json

ROOT = Path(__file__).resolve().parents[2]


def load(name):
    spec = importlib.util.spec_from_file_location(name, ROOT / 'scripts' / (name + '.py'))
    mod = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(mod)
    return mod


facts = load('ci-run-facts')
rules = load('merge-queue-ruleset')


class Recovery(unittest.TestCase):
    def setUp(self):
        self.run = dict(name='main post-merge', head_branch='main', event='push',
                        head_repository={'full_name': 'stave-dsp/stave'}, conclusion='failure')

    def test_only_trusted_main_failures(self):
        self.assertTrue(facts.eligible(self.run, 'stave-dsp/stave'))
        for key, value in [('head_branch', 'feature'), ('event', 'pull_request'),
                           ('name', 'ci'), ('conclusion', 'cancelled'),
                           ('head_repository', {'full_name': 'fork/stave'})]:
            r = {**self.run, key: value}
            self.assertFalse(facts.eligible(r, 'stave-dsp/stave'))

    def test_schedule_and_manual_main_failures(self):
        for event in ['schedule', 'workflow_dispatch']:
            self.assertTrue(facts.eligible({**self.run, 'event': event}, 'stave-dsp/stave'))

    def test_duration_and_missing_timestamp(self):
        self.assertEqual(facts.seconds('2026-11-14T00:00:00Z', '2026-11-14T00:01:02Z'), 62)
        self.assertIsNone(facts.seconds(None, '2026-11-14T00:01:02Z'))

    def test_facts_survive_unreachable_control_and_paginate_jobs(self):
        r = {**self.run, 'id': 123, 'run_attempt': 2, 'head_sha': 'a'*40,
             'html_url': 'https://example.test/run', 'created_at': '2026-11-14T00:00:00Z',
             'run_started_at': '2026-11-14T00:01:00Z', 'updated_at': '2026-11-14T00:02:00Z'}
        job = dict(name='miri', conclusion='failure', created_at=r['created_at'],
                   started_at=r['run_started_at'], completed_at=r['updated_at'])
        with tempfile.TemporaryDirectory() as d:
            old = os.getcwd()
            try:
                os.chdir(d)
                Path('event.json').write_text(json.dumps({'workflow_run': r}))
                env = {'GITHUB_REPOSITORY': 'stave-dsp/stave', 'GITHUB_EVENT_PATH': 'event.json',
                       'GITHUB_OUTPUT': 'out', 'CONTROL_EVENTS_URL': 'https://example.test/api/events'}
                with patch.dict(os.environ, env), patch.object(facts, 'api', side_effect=[r, {'jobs': [job]*100}, {'jobs': [job]}]), patch.object(facts.urllib.request, 'urlopen', side_effect=OSError('offline')):
                    facts.main()
                self.assertEqual(len(json.loads(Path('ci-run-facts.json').read_text())['checks']), 101)
                self.assertEqual(json.loads(Path('control-delivery.json').read_text())['state'], 'failed')
                self.assertIn('revert_sha='+'a'*40, Path('out').read_text())
                self.assertEqual(json.loads(Path('control-event.json').read_text())['kind'], 'note')
            finally:
                os.chdir(old)


class Ruleset(unittest.TestCase):
    def test_preserves_every_other_policy(self):
        current = dict(name='main', target='branch', enforcement='active', bypass_actors=[],
                       conditions={'ref_name': {'include': ['~DEFAULT_BRANCH'], 'exclude': []}},
                       rules=[{'type': 'merge_queue', 'parameters': {'max_entries_to_build': 5}},
                              {'type': 'non_fast_forward'},
                              {'type': 'required_status_checks', 'parameters': {
                                  'strict_required_status_checks_policy': False,
                                  'required_status_checks': [{'context': 'old'}]}}])
        before = copy.deepcopy(current)
        result = rules.reshape(current)
        self.assertEqual(current, before)
        self.assertEqual(result['rules'][:2], current['rules'][:2])
        checks = result['rules'][2]['parameters']['required_status_checks']
        self.assertEqual([c['context'] for c in checks], rules.REQUIRED)

    def test_rejects_unexpected_policy(self):
        with self.assertRaises(ValueError):
            rules.reshape({'target': 'tag', 'enforcement': 'active'})


class RevertOrdering(unittest.TestCase):
    def exercise(self, failure):
        import subprocess
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / 'bin').mkdir()
            (root / 'scripts').mkdir()
            fake = root / 'bin' / 'fake'
            fake.write_text(r"""#!/usr/bin/env python3
import json, os, pathlib, sys
name = pathlib.Path(sys.argv[0]).name
args = sys.argv[1:]
with open(os.environ['CALL_LOG'], 'a') as log:
    log.write(json.dumps([name, *args]) + '\n')
if name == 'cargo' and args[0] == os.environ.get('FAIL_COMMAND'):
    sys.exit(47)
if name == 'git' and args[:3] == ['show', '-s', '--format=%s']:
    print('A source change')
if name == 'git' and args[:3] == ['show', '-s', '--format=%P']:
    print('b'*40)
if name == 'git' and args == ['diff', '--cached', '--quiet']:
    sys.exit(1)
""")
            fake.chmod(0o755)
            for name in ['git', 'gh', 'cargo']:
                (root / 'bin' / name).symlink_to(fake)
            leak = root / 'scripts' / 'check-local-leaks.sh'
            leak.write_text('#!/bin/sh\nexit 0\n')
            leak.chmod(0o755)
            env = {**os.environ, 'PATH': str(root / 'bin') + os.pathsep + os.environ['PATH'],
                   'CALL_LOG': str(root / 'calls'), 'FAIL_COMMAND': failure,
                   'FAILED_SHA': 'a'*40, 'FAILED_RUN_URL': 'https://example.test/run',
                   'GITHUB_REPOSITORY': 'stave-dsp/stave'}
            result = subprocess.run(['bash', str(ROOT / 'scripts' / 'ci-revert.sh')],
                                    cwd=root, env=env, capture_output=True, text=True)
            calls = [json.loads(line) for line in (root / 'calls').read_text().splitlines()]
            return result, calls

    def test_failed_verification_never_commits_or_pushes(self):
        for command in ['build', 'nextest', 'test']:
            result, calls = self.exercise(command)
            self.assertNotEqual(result.returncode, 0)
            self.assertFalse(any('commit' in c or 'push' in c for c in calls), calls)

    def test_success_verifies_before_commit_and_never_merges(self):
        result, calls = self.exercise('none')
        self.assertEqual(result.returncode, 0, result.stderr)
        commit = next(i for i, c in enumerate(calls) if 'commit' in c)
        for command in ['build', 'nextest', 'test']:
            self.assertLess(next(i for i, c in enumerate(calls) if c[:2] == ['cargo', command]), commit)
        self.assertFalse(any(c[:3] == ['gh', 'pr', 'merge'] for c in calls))
        self.assertIn('Co-Authored-By: Codex <noreply@openai.com>', calls[commit])


class Selection(unittest.TestCase):
    def exercise(self, root_change):
        import shutil
        import subprocess
        with tempfile.TemporaryDirectory() as d:
            root = Path(d)
            (root / 'scripts').mkdir()
            (root / 'crates' / 'p').mkdir(parents=True)
            (root / 'Cargo.toml').write_text('rust-version = "1.86"\n')
            shutil.copyfile(ROOT / 'scripts' / 'affected.sh', root / 'scripts' / 'affected.sh')
            (root / 'scripts' / 'test-affected.py').write_text('raise SystemExit(42)\n')
            (root / 'crates' / 'p' / 'lib.rs').write_text('// before\n')
            subprocess.run(['git', 'init', '-q'], cwd=root, check=True)
            subprocess.run(['git', 'add', '.'], cwd=root, check=True)
            subprocess.run(['git', '-c', 'user.name=CI fixture', '-c', 'user.email=fixture@example.test',
                            '-c', 'core.hooksPath=/dev/null', 'commit', '-qm',
                            'Fixture\n\nCo-Authored-By: Codex <noreply@openai.com>'], cwd=root, check=True)
            changed = root / 'new-root-config' if root_change else root / 'crates' / 'p' / 'lib.rs'
            changed.write_text('changed\n')
            # Keep command fixtures outside the tree: untracked root files
            # would legitimately select the full run and hide the failure.
            with tempfile.TemporaryDirectory() as commands:
                cargo = Path(commands) / 'cargo'
                metadata = json.dumps({'packages': [{'name': 'p', 'targets': [{'kind': ['lib']}]}]})
                cargo.write_text('#!/usr/bin/env python3\nprint(' + repr(metadata) + ')\n')
                cargo.chmod(0o755)
                env = {**os.environ, 'PATH': commands + os.pathsep + os.environ['PATH']}
                return subprocess.run(['bash', 'scripts/affected.sh', '--outputs', '--base', 'HEAD'],
                                      cwd=root, env=env, capture_output=True, text=True)

    def test_selector_error_cannot_turn_into_empty_success(self):
        self.assertEqual(self.exercise(False).returncode, 42)

    def test_any_root_file_forces_full_run(self):
        result = self.exercise(True)
        self.assertEqual(result.returncode, 0, result.stderr)
        self.assertIn('AFFECTED_SCOPE=workspace', result.stdout)


class ActionPins(unittest.TestCase):
    def test_remote_actions_have_full_sha_and_version_comment(self):
        import re
        for path in (ROOT / '.github').rglob('*.yml'):
            for line in path.read_text().splitlines():
                match = re.search(r'uses: (\S+)', line)
                if match and not match[1].startswith('./'):
                    self.assertRegex(line, r'uses: [\w/-]+@[a-f0-9]{40} # v[\w.-]+', str(path))


if __name__ == '__main__':
    unittest.main()
