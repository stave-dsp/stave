# Operating the merge queue

Changes land through a pull request and GitHub's merge queue. Enqueue with
`gh pr merge --auto NUMBER`; never push directly to main, bypass protection,
or delete an agent's branch or worktree. A green PR is feedback on its base;
the merge group checks the candidate that will actually land.

## Required checks

The proposed ruleset requires three checks:

| Check | Work it includes |
| --- | --- |
| `queue gate` | Three macOS nextest shards, doctests, clippy/rustdoc/MSRV, macOS format validators, tree rules and dependency hygiene, portable builds and loom |
| `platforms gate` | Linux and Windows builds, tests, doctests, clippy/rustdoc and format validators |
| `build the site` | Documentation build, examples, links and generated pages |

Both aggregate gates run even after a dependency fails or is cancelled and
require every affected dependency to succeed. The platform gate accepts skipped
jobs only after successful selection explicitly reports no affected packages;
a missing, failed or cancelled affected job never counts as a pass. The downstream
`plugins against this change (Linux)` job remains advisory.

PRs and merge groups use `scripts/affected.sh` against their base. It selects
changed packages and their dependents; all root files, shared configuration,
scripts and workflows force a full run. Selection errors fail closed. Main
pushes and manual dispatches always run fully. Platform validators and loom
remain conservative when any package is affected. PR runs are cancelled on
superseding pushes; merge groups retain GitHub's candidate lifecycle. We do not
reuse a PR pass for a different merge candidate.

macOS tests use three hash partitions; doctests run once. This changes parallelism,
not coverage. Miri, both circuit T1 suites, rate-range T1, fuzz targets and Kani
harnesses run in `main-postmerge.yml` for every main push, nightly and on demand.
There are no path filters or concurrency cancellation that can silently drop a
main revision. Reference regeneration stays in `references.yml`, retaining its
existing affected deck selection and self-hosted routing. Sanitizers, the wide
simulated host sweep, advisories and release semver checks stay in `deep.yml`.
No blanket retries or new test time limits are introduced.

## Apply the required-check migration

Land the workflow PR first. Until the human applies the migration, the original
required names remain available: `test (macOS)` aggregates the new shards and
the four old Miri names are explicitly migration bridges to `queue gate`.
They **do not run Miri**; actual Miri results belong to `main post-merge`.
Do not interpret a bridge pass as a Miri result. Remove bridges in a follow-up
PR after confirming no ruleset still requires them.

Prepare a fresh policy proposal; the script only reads GitHub and writes local
before/after JSON. It preserves queue parameters, bypass actors, review rules,
and every policy other than the required-check list:

```sh
python3 scripts/merge-queue-ruleset.py
diff -u target/merge-queue-ruleset.before.json target/merge-queue-ruleset.json
```

Review the three check names on successful PR and merge-group runs. Confirm the
live ruleset has not changed since the snapshot, then the human applies:

```sh
gh api --method PUT repos/stave-dsp/stave/rulesets/27218038 --input target/merge-queue-ruleset.json
```

Fetch the ruleset again to verify. Do not apply a stale full-ruleset payload over
someone else's policy edit. The old snapshot is the rollback proposal, not an
automatic rollback command. No workflow changes rulesets.

## Post-merge recovery

`ci-observe.yml` listens for completed runs, checks out trusted main, and reads
GitHub's run/job API. It never executes a PR checkout or downloaded artifact.
On a failed `main post-merge` run from this repository's main branch, it emits a
control event and prepares a revert against current main. The failed SHA must
still be an ancestor. Existing revert PRs are reused, including closed ones;
a failing revert is escalated instead of starting a revert loop.

Before committing the revert, the recovery job checks the patch, builds all
targets, runs nextest and doctests, and checks for local configuration leaks.
It opens a PR and dispatches CI, platform and documentation checks on that branch.
It never merges the revert. Review whether the failure was caused by the revision
or by infrastructure, especially for scheduled failures. A conflict, validation
failure or unavailable permission opens a fix-forward issue with the run links.
If issue creation itself fails, the recovery workflow stays red with its error.
Do not repeatedly requeue without understanding the failure.

GitHub must allow Actions to create pull requests. An optional `CI_REVERT_TOKEN`
secret can supply a bot token with repository contents, pull-request, issues and
Actions permissions (and workflow write permission when a revert touches workflow
files). Without it, `GITHUB_TOKEN` is used; explicit workflow dispatches avoid the
normal suppression of PR-triggered runs created by that token. Repository policy
can still deny writes: inspect the failed recovery step and prepare the PR manually.

## Facts and control delivery

Each completed run gets a `ci-facts-RUN-ATTEMPT` artifact in the observer workflow:

- `ci-run-facts.json`: workflow, event, ref, SHA, result, queue wait and elapsed
  duration, plus each check's result, queue wait and duration. Missing timestamps
  are null, never fabricated zeroes. Jobs are paginated, and retries are tied to
  the triggering attempt.
- `control-event.json`: a supported control `note` event; failed post-merge runs
  are titled `POST-MERGE FAILURE` with outcome `failed`.
- `control-delivery.json`: delivered, unconfigured or failed, so loss is visible.

Configure secret `CONTROL_EVENTS_URL` with the reachable complete `/api/events`
URL and optional `CONTROL_EVENTS_TOKEN` for an authenticated gateway. Never put
an internal address or token in public files. GitHub-hosted runners must actually
be able to reach this endpoint; an internal-only endpoint needs an approved relay.
The collector checks that control accepted exactly one event without errors.
Missing configuration or delivery failure emits a workflow warning and retains
the event for replay. A control outage does not prevent recovery. Replay the
JSON artifact via POST with content type `application/json`; use the same event ID.
The current control API accepts `note`, not an invented `ci.run` event kind.

## Pause, dequeue, diagnose and requeue

Use control's `ci.merge_queue` kill switch with an owner and reason to pause
control-driven intake. It does not prevent manual GitHub enqueue operations and
does not halt already queued entries. A complete administrative pause requires
a reviewed ruleset change; export the policy first and let the human apply it.
Do not remove required checks as an incident workaround.

Read queue depth, entry age and ordering (paginate `entries` using `pageInfo`):

```sh
gh api graphql -f query='query {
  repository(owner:"stave-dsp", name:"stave") {
    mergeQueue(branch:"main") {
      entries(first:100) {
        totalCount
        nodes { position enqueuedAt state pullRequest { number title url } }
        pageInfo { hasNextPage endCursor }
      }
    }
  }
}'
```

Record the candidate SHA, base SHA, entries, failing check and run URL before
adjusting a failed batch. Inspect `gh pr checks NUMBER` and `gh run view RUN_ID
--log-failed`. Reproduce on the candidate when possible. Dequeue the suspect
through control's audited action or GitHub's queue UI, recording the reason.
Requeue unaffected entries in the intended order and enqueue a fix/revert through
`gh pr merge --auto NUMBER`. GitHub owns speculative batching and ejection; this
workflow does not attempt to reorder its internal queue or bypass FIFO.

## Flakes and measurement

Control's per-repository `ci` configuration supports quarantine entries with
`check`, `reason`, `owner` and `until` (an ISO date). Quarantine changes control's
classification only; it does not waive GitHub's required checks. Expired entries
return to their real classification. Any required-check change needs the same
reviewed policy process. Add a nextest retry only for a demonstrated flaky test,
with an exact filter and owner comment; there are no global retries here.

Compare doc-only, one-crate and full-change PRs, then their merge groups. Separate
runner queue wait from execution time, report the slowest required job, and keep
run IDs with the measurement. Workflow elapsed time includes dispatch and job
queueing; it is not the sum of job durations. Compare p50/p95 only after collecting
enough runs. SHA pins are updated by weekly Dependabot PRs and pass the same gates.
