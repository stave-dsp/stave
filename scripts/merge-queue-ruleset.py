#!/usr/bin/env python3
"""Prepare (never apply) a required-check update from a fresh ruleset snapshot."""
import argparse
import copy
import json
from pathlib import Path
import subprocess

REQUIRED = ['queue gate', 'platforms gate', 'build the site']


def reshape(current):
    if current.get('target') != 'branch' or current.get('enforcement') != 'active':
        raise ValueError('expected an active branch ruleset')
    rules = [r for r in current['rules'] if r['type'] == 'required_status_checks']
    if len(rules) != 1 or not any(r['type'] == 'merge_queue' for r in current['rules']):
        raise ValueError('expected one required-check rule and a merge queue')
    result = {k: copy.deepcopy(current[k]) for k in
              ('name', 'target', 'enforcement', 'bypass_actors', 'conditions', 'rules')}
    rule = next(r for r in result['rules'] if r['type'] == 'required_status_checks')
    rule['parameters']['required_status_checks'] = [
        {'context': name, 'integration_id': 15368} for name in REQUIRED]
    return result


def main():
    p = argparse.ArgumentParser(description=__doc__)
    p.add_argument('--repo', default='stave-dsp/stave')
    p.add_argument('--ruleset', type=int, default=27218038)
    p.add_argument('--output', type=Path, default=Path('target/merge-queue-ruleset.json'))
    args = p.parse_args()
    endpoint = f'repos/{args.repo}/rulesets/{args.ruleset}'
    raw = subprocess.check_output(['gh', 'api', endpoint], text=True)
    result = reshape(json.loads(raw))
    encoded = json.dumps(result, indent=2) + '\n'
    assert json.loads(encoded) == result
    args.output.parent.mkdir(parents=True, exist_ok=True)
    args.output.with_suffix('.before.json').write_text(raw + '\n')
    args.output.write_text(encoded)
    import shlex
    print('Review the before/after files. Only a human applies:')
    print(f'gh api --method PUT {shlex.quote(endpoint)} --input {shlex.quote(str(args.output))}')


if __name__ == '__main__':
    main()
