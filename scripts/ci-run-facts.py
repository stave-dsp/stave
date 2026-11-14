#!/usr/bin/env python3
"""Collect completed run facts, propose a revert, and notify control.

Executed only from the default branch by workflow_run, never from the
completed run's checkout or artifacts. No repository build code executes.
"""
import datetime as dt
import json
import os
from pathlib import Path
import re
import subprocess
import urllib.request


def run(*args):
    return subprocess.check_output(args, text=True).strip()


def api(path):
    return json.loads(run('gh', 'api', path))


def seconds(start, end):
    if not start or not end:
        return None
    return (dt.datetime.fromisoformat(end.replace('Z', '+00:00')) -
            dt.datetime.fromisoformat(start.replace('Z', '+00:00'))).total_seconds()


def eligible(r, repo):
    return (r['name'] == 'main post-merge' and r['head_branch'] == 'main'
            and r['event'] in ('push', 'schedule', 'workflow_dispatch')
            and r['head_repository']['full_name'] == repo
            and r['conclusion'] == 'failure')


def main():
    repo = os.environ['GITHUB_REPOSITORY']
    event = json.loads(Path(os.environ['GITHUB_EVENT_PATH']).read_text())
    source = event['workflow_run']
    r = api(f"repos/{repo}/actions/runs/{source['id']}/attempts/{source['run_attempt']}")
    jobs = []
    page = 1
    while True:
        batch = api(f"repos/{repo}/actions/runs/{r['id']}/attempts/{r['run_attempt']}/jobs?per_page=100&page={page}")['jobs']
        jobs.extend(batch)
        if len(batch) < 100:
            break
        page += 1
    facts = dict(schema=1, repository=repo, run_id=r['id'], attempt=r['run_attempt'],
                 workflow=r['name'], event=r['event'], ref=r['head_branch'],
                 queue_wait_seconds=seconds(r['created_at'], r['run_started_at']), sha=r['head_sha'], result=r['conclusion'], url=r['html_url'],
                 duration_seconds=seconds(r['run_started_at'], r['updated_at']),
                 checks=[dict(name=j['name'], result=j['conclusion'],
                              queue_wait_seconds=seconds(j.get('created_at'), j['started_at']),
                              duration_seconds=seconds(j['started_at'], j['completed_at'])) for j in jobs])
    Path('ci-run-facts.json').write_text(json.dumps(facts, indent=2) + '\n')
    failed = eligible(r, repo)
    if failed:
        if not re.fullmatch(r'[0-9a-f]{40}', r['head_sha']):
            raise ValueError('invalid failed revision')
        with open(os.environ['GITHUB_OUTPUT'], 'a') as f:
            f.write(f"revert_sha={r['head_sha']}\n")
    note = dict(kind='note', project=repo.split('/')[-1],
                id=f"ci-{r['id']}-{r['run_attempt']}",
                title=f"{'POST-MERGE FAILURE' if failed else 'CI'}: {r['name']} {r['conclusion']}",
                detail=json.dumps(facts), at=int(dt.datetime.now(dt.timezone.utc).timestamp()*1000),
                url=r['html_url'], outcome='failed' if failed else None)
    Path('control-event.json').write_text(json.dumps(note) + '\n')
    endpoint = os.environ.get('CONTROL_EVENTS_URL')
    delivery = {'state': 'unconfigured'}
    if endpoint:
        try:
            headers = {'Content-Type': 'application/json'}
            token = os.environ.get('CONTROL_EVENTS_TOKEN')
            if token:
                headers['Authorization'] = f'Bearer {token}'
            req = urllib.request.Request(endpoint, data=json.dumps(note).encode(), headers=headers)
            with urllib.request.urlopen(req, timeout=20) as response:
                receipt = json.load(response)
                if receipt.get('accepted') != 1 or receipt.get('errors'):
                    raise ValueError('control rejected the event')
            delivery = {'state': 'delivered'}
        except Exception as exc:
            delivery = {'state': 'failed', 'error_type': type(exc).__name__}
    Path('control-delivery.json').write_text(json.dumps(delivery) + '\n')
    if delivery['state'] != 'delivered':
        print(f"::warning::Control delivery {delivery['state']}; replay control-event.json from artifacts")


if __name__ == '__main__':
    main()
