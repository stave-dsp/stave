#!/usr/bin/env bash
# Only called from trusted main by ci-observe.yml. Never merges a revert.
set -euo pipefail
[[ "$FAILED_SHA" =~ ^[0-9a-f]{40}$ ]]
branch="ci/revert-deep-$FAILED_SHA"
existing="$(gh pr list --repo "$GITHUB_REPOSITORY" --head "$branch" --state all --json url --jq '.[].url')"
if [[ -n "$existing" ]]; then
  echo "Revert already proposed: $existing"
  exit 0
fi
git fetch origin main
git merge-base --is-ancestor "$FAILED_SHA" origin/main
subject="$(git show -s --format=%s "$FAILED_SHA")"
if [[ "$subject" == Revert* ]]; then
  echo '::error::Failed revision is a revert; refusing a revert loop. Human review required.'
  exit 1
fi
git switch -c "$branch" origin/main
parent_line="$(git show -s --format=%P "$FAILED_SHA")"
read -r -a parents <<<"$parent_line"
args=()
((${#parents[@]} > 1)) && args=(-m 1)
git revert --no-commit ${args[@]+"${args[@]}"} "$FAILED_SHA"
git diff --cached --check
if git diff --cached --quiet; then
  echo '::error::No revert diff remains; inspect current main.'
  exit 1
fi
# Verify before committing, per repository policy. Every error stops here.
cargo build --workspace --all-targets --locked
cargo nextest run --workspace --locked --profile ci
cargo test --workspace --doc --locked
scripts/check-local-leaks.sh --staged
git config user.name 'stave CI'
git config user.email 'noreply@openai.com'
git -c core.hooksPath=/dev/null commit -m "Revert failed post-merge revision $FAILED_SHA" -m "Failure: $FAILED_RUN_URL" -m 'Co-Authored-By: Codex <noreply@openai.com>'
git push origin "HEAD:refs/heads/$branch"
body="$(mktemp)"
trap 'rm -f "$body"' EXIT
printf 'Post-merge deep checks failed: %s\n\nProposes reverting %s on current main. This is a candidate, not proof of fault attribution (scheduled failures may be environmental). Build, nextest and doctests passed before committing. Review the failure and rerun deep checks before enqueueing.\n' "$FAILED_RUN_URL" "$FAILED_SHA" > "$body"
gh pr create --repo "$GITHUB_REPOSITORY" --head "$branch" --base main --title "Revert failing deep-check revision ${FAILED_SHA:0:12}" --body-file "$body"
# GITHUB_TOKEN-created PRs do not start pull_request workflows. Explicit
# dispatch makes checks run on the candidate even without a bot token.
for workflow in ci.yml platforms.yml docs.yml; do
  gh workflow run "$workflow" --repo "$GITHUB_REPOSITORY" --ref "$branch"
done
