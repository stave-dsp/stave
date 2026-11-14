#!/usr/bin/env bash
# Lifecycle cleanup on the coordinating machine: what finished agents leave
# behind. Lists by default; changes nothing without --apply (worktrees
# only), and prints the exact command for anything it will not do itself.
#
#   scripts/janitor.sh                      # all three reports
#   scripts/janitor.sh worktrees [--apply] [--include-locked] [--idle HOURS]
#   scripts/janitor.sh branches             # remote agent branches
#   scripts/janitor.sh shells [--hours N]   # stale polling shells, stale ssh muxes
#
# worktrees  Agent worktrees (.claude/worktrees/agent-*). Removable when
#            nothing would be lost and nothing uses it: no uncommitted
#            changes, no process working in it, and its branch is merged —
#            every commit on origin/main, or its pull request merged at
#            exactly the branch's tip (squash merges) — or its pull request
#            closed with the tip pushed (the commits stay on the remote; the
#            local branch is kept). A branch with no commits of its own and
#            no pull request may belong to an agent still starting; it counts
#            only with --idle HOURS (untouched for that long). A worktree
#            locked by a live session (Claude Code locks the worktrees of
#            its agents) needs --include-locked. --apply removes each
#            removable worktree (`git worktree remove`; its target/ goes
#            with it) and deletes its local branch when merged.
# branches   Remote agent branches (agent-*, worktree-agent-*,
#            coordinator/*, <agent id>/*) whose pull request is merged (or
#            whose commits are all on origin/main): never deleted here —
#            prints the `git push origin --delete` command. Closed-unmerged
#            branches are listed apart (their commits are on no other branch).
# shells     Shells that Claude Code started (their command line sources a
#            .claude/shell-snapshots file) running longer than N hours
#            (default 6; 0: all) that only wait in a polling loop
#            (until/while … sleep, or tail -f), and stopped ssh
#            multiplexers (which can hold sessions open on build hosts).
#            Never killed here: prints the `kill` command.
#
# Needs git and gh (authenticated) for worktrees and branches.
set -euo pipefail
cmd="${1:-all}"
[[ $# -gt 0 ]] && shift
APPLY=0 LOCKED=0 IDLE="" HOURS=6 FETCH=1
while (($#)); do
  case "$1" in
    --apply) APPLY=1 ;;
    --include-locked) LOCKED=1 ;;
    --idle) IDLE="${2:?--idle needs hours}"; shift ;;
    --hours) HOURS="${2:?--hours needs a number}"; shift ;;
    --no-fetch) FETCH=0 ;;
    --dry-run) APPLY=0 ;;
    *) echo "janitor: unknown option $1" >&2; exit 2 ;;
  esac
  shift
done
[[ -z "$IDLE" || "$IDLE" =~ ^[0-9]+$ ]] || { echo "janitor: --idle takes whole hours" >&2; exit 2; }
[[ "$HOURS" =~ ^[0-9]+$ ]] || { echo "janitor: --hours takes whole hours" >&2; exit 2; }

ROOT="$(git rev-parse --show-toplevel)"
# The main checkout: worktree paths and git commands go through it.
MAIN="$(cd "$(git -C "$ROOT" rev-parse --path-format=absolute --git-common-dir)/.." && pwd -P)"
G() { git -C "$MAIN" "$@"; }
SCRATCH="$(mktemp -d "${TMPDIR:-/tmp}/janitor.XXXXXX")"
trap 'rm -rf "$SCRATCH"' EXIT

# Pull requests, every one (paginated, no cap): number, state, merged at,
# head branch, head sha.
pulls() {
  [[ -f "$SCRATCH/pulls" ]] && return 0
  gh api --paginate 'repos/{owner}/{repo}/pulls?state=all&per_page=100' \
    --jq '.[] | [.number, .state, (.merged_at // "-"), .head.ref, .head.sha] | @tsv' > "$SCRATCH/pulls" \
    || { echo "janitor: could not list pull requests (gh)" >&2; exit 1; }
}
fetch() {
  ((FETCH)) || return 0
  [[ -f "$SCRATCH/fetched" ]] && return 0
  G fetch -q --prune origin || echo "janitor: fetch failed; using the last fetched origin/main" >&2
  : > "$SCRATCH/fetched"
}
# The pull request verdict for BRANCH at TIP: "open #N", "merged #N"
# (merged at exactly TIP), "merged-other #N" (merged, at another commit),
# "closed #N" (closed unmerged at TIP), "closed-other #N", or "none".
pr_state() {
  local branch="$1" tip="$2"
  awk -F'\t' -v b="$branch" -v t="$tip" '
    $4 == b {
      if ($2 == "open") open = "open #" $1
      else if ($3 != "-") { if ($5 == t) m = "merged #" $1; else if (!mo) mo = "merged-other #" $1 }
      else { if ($5 == t) c = "closed #" $1; else if (!co) co = "closed-other #" $1 }
    }
    END { print open ? open : m ? m : c ? c : mo ? mo : co ? co : "none" }' "$SCRATCH/pulls"
}
age() { local s=$1; if ((s >= 86400)); then printf '%dd%02dh' $((s / 86400)) $((s % 86400 / 3600)); else printf '%dh%02dm' $((s / 3600)) $((s % 3600 / 60)); fi; }
mtime() { stat -f %m "$1" 2> /dev/null || stat -c %Y "$1" 2> /dev/null || echo 0; }

worktrees() {
  fetch
  pulls
  local now
  now="$(date +%s)"
  # Working directories of this user's processes: "pid<TAB>cwd".
  lsof -a -d cwd -u "$(id -u)" -Fpn 2> /dev/null \
    | awk '/^p/ { p = substr($0, 2) } /^n/ { print p "\t" substr($0, 2) }' > "$SCRATCH/cwds" || true
  # Reading status must not rewrite a worktree's index (its mtime is
  # how idle the worktree is).
  export GIT_OPTIONAL_LOCKS=0
  G worktree list --porcelain > "$SCRATCH/wt"
  local path="" head="" branch="" locked="" n_rm=0 n_keep=0 freed=0
  local -a rows=()
  flush() {
    [[ -n "$path" && "$(basename "$path")" == agent-* && "$path" == */.claude/worktrees/* ]] || return 0
    local why="" verdict="keep" kb=0 last gitdir pr ahead lockpid="" lockalive=0 users
    gitdir="$(git -C "$path" rev-parse --path-format=absolute --git-dir 2> /dev/null || true)"
    last=0
    for f in "$gitdir/HEAD" "$gitdir/index" "$gitdir/logs/HEAD"; do
      local m; m="$(mtime "$f")"; ((m > last)) && last=$m
    done
    if [[ -n "$locked" ]]; then
      lockpid="$(sed -n 's/.*(pid \([0-9]*\).*/\1/p' <<< "$locked")"
      [[ -n "$lockpid" ]] && kill -0 "$lockpid" 2> /dev/null && lockalive=1
    fi
    if [[ ! -d "$path" ]]; then
      verdict=remove why="directory is gone (prune)"
    elif [[ -n "$(git -C "$path" status --porcelain 2> /dev/null | head -1)" ]]; then
      why="uncommitted changes"
    elif users="$(awk -F'\t' -v p="$path" '$2 == p || index($2, p "/") == 1 { print $1 }' "$SCRATCH/cwds" | sort -n | tr '\n' ' ')" \
      && [[ -n "$users" ]]; then
      why="processes work in it: pid ${users% } (janitor.sh shells lists stale ones)"
    elif [[ -z "$branch" ]]; then
      why="detached HEAD"
    else
      ahead="$(G rev-list --count "origin/main..$head")"
      pr="$(pr_state "$branch" "$head")"
      case "$pr" in
        open*) why="pull request ${pr#open } is open" ;;
        merged\ *) verdict=remove why="pull request ${pr#merged } merged at its tip" ;;
        closed\ *)
          if [[ "$(G rev-parse -q --verify "refs/remotes/origin/$branch" 2> /dev/null)" == "$head" ]]; then
            verdict=remove why="pull request ${pr#closed } closed; its tip is on origin (branch kept)"
          else
            why="pull request ${pr#closed } closed and its tip is not pushed: $ahead commits only here"
          fi ;;
        *)
          if ((ahead > 0)); then
            why="$ahead commits not on origin/main (${pr})"
          elif [[ "$pr" == merged-other* ]]; then
            verdict=remove why="its commits are on origin/main (pull request ${pr#merged-other })"
          elif [[ -n "$IDLE" ]] && ((now - last >= IDLE * 3600)); then
            verdict=remove why="no commits of its own, no pull request, untouched $(age $((now - last)))"
          else
            why="no commits of its own and no pull request: may be starting (--idle HOURS counts it)"
          fi ;;
      esac
    fi
    if [[ "$verdict" == remove ]] && ((lockalive && !LOCKED)); then
      verdict=keep why="$why; locked by live session pid $lockpid (--include-locked)"
    fi
    if [[ "$verdict" == remove && -d "$path" ]]; then
      kb="$(du -sk "$path" 2> /dev/null | awk '{print $1}')"
    fi
    if [[ "$verdict" == remove ]]; then
      n_rm=$((n_rm + 1)); freed=$((freed + kb))
      if ((APPLY)); then
        if [[ -d "$path" ]]; then
          local force=()
          [[ -n "$locked" ]] && force=(-f -f)
          if G worktree remove "${force[@]}" "$path"; then
            verdict=removed
            # The branch goes too when nothing on it is unmerged.
            if [[ -n "$branch" && "$why" != *"branch kept"* ]]; then
              G branch -D "$branch" > /dev/null 2>&1 || true
            fi
          else
            verdict="failed"
          fi
        fi
      else
        verdict="would remove"
      fi
    else
      n_keep=$((n_keep + 1))
    fi
    rows+=("$(printf '%-13s %-28s %-7s %6s  %s' "$verdict" "$(basename "$path")" "$(age $((now - last)))" \
      "$( ((kb)) && awk -v k="$kb" 'BEGIN { printf "%.1fG", k / 1048576 }')" "${branch:-?}: $why")")
  }
  while IFS= read -r line; do
    case "$line" in
      "worktree "*) flush; path="${line#worktree }" head="" branch="" locked="" ;;
      "HEAD "*) head="${line#HEAD }" ;;
      "branch "*) branch="${line#branch refs/heads/}" ;;
      locked*) locked="${line#locked}"; locked="${locked:- locked}" ;;
    esac
  done < "$SCRATCH/wt"
  flush
  echo "== agent worktrees (verdict, name, idle, size, branch: why)"
  ((${#rows[@]})) && printf '%s\n' "${rows[@]}" | sort
  ((APPLY)) && G worktree prune
  printf '%s: %d removable (%.1f GB), %d kept%s\n' "$( ((APPLY)) && echo removed || echo "dry run")" \
    "$n_rm" "$(awk -v k="$freed" 'BEGIN { print k / 1048576 }')" "$n_keep" "$( ((APPLY)) || echo '; --apply removes them')"
}

branches() {
  fetch
  pulls
  local -a del=() closed=() keep=()
  while read -r sha ref; do
    local b="${ref#refs/heads/}" pr ahead
    [[ "$b" =~ ^(agent-|worktree-agent-|coordinator/|a[0-9a-f]{7,17}/) ]] || continue
    pr="$(pr_state "$b" "$sha")"
    ahead="$(G rev-list --count "origin/main..$sha" 2> /dev/null || echo '?')"
    case "$pr" in
      merged\ *) del+=("$b	pull request ${pr#merged } merged at its tip") ;;
      open*) keep+=("$b	pull request ${pr#open } open") ;;
      *)
        if [[ "$ahead" == 0 ]]; then del+=("$b	every commit is on origin/main ($pr)")
        elif [[ "$pr" == closed* ]]; then closed+=("$b	pull request ${pr#closed* } closed unmerged; $ahead commits on no other branch")
        else keep+=("$b	$ahead commits not on origin/main ($pr)"); fi ;;
    esac
  done < <(G ls-remote --heads origin)
  echo "== remote agent branches"
  local x
  for x in "${del[@]+"${del[@]}"}"; do printf 'deletable     %s\n' "${x/	/  }"; done
  for x in "${closed[@]+"${closed[@]}"}"; do printf 'closed        %s\n' "${x/	/  }"; done
  for x in "${keep[@]+"${keep[@]}"}"; do printf 'keep          %s\n' "${x/	/  }"; done
  echo "${#del[@]} deletable, ${#closed[@]} closed unmerged, ${#keep[@]} kept"
  if ((${#del[@]})); then
    printf 'delete the deletable ones (needs your OK):\n  git push origin --delete'
    for x in "${del[@]}"; do printf ' %s' "${x%%	*}"; done
    echo
  fi

  # Local branches no worktree has checked out: merged ones (every commit on
  # origin/main, or the pull request merged at exactly the tip) lose nothing
  # when deleted; --apply deletes them. The rest stay.
  local -a ldel=() lkeep=()
  while read -r b sha wt; do
    [[ -z "$wt" && "$b" != main && "$b" != release/* ]] || continue
    local pr ahead
    pr="$(pr_state "$b" "$sha")"
    ahead="$(G rev-list --count "origin/main..$sha")"
    if [[ "$pr" == merged\ * ]]; then ldel+=("$b	pull request ${pr#merged } merged at its tip")
    elif [[ "$ahead" == 0 && "$pr" != open* ]]; then ldel+=("$b	every commit is on origin/main")
    else lkeep+=("$b	$ahead commits not on origin/main ($pr)"); fi
  done < <(G for-each-ref refs/heads --format='%(refname:short) %(objectname) %(worktreepath)')
  echo
  echo "== local branches outside any worktree"
  for x in "${ldel[@]+"${ldel[@]}"}"; do
    if ((APPLY)) && G branch -D "${x%%	*}" > /dev/null 2>&1; then printf 'deleted       %s\n' "${x/	/  }"
    else printf '%-13s %s\n' "$( ((APPLY)) && echo failed || echo deletable)" "${x/	/  }"; fi
  done
  for x in "${lkeep[@]+"${lkeep[@]}"}"; do printf 'keep          %s\n' "${x/	/  }"; done
  echo "${#ldel[@]} merged$( ((APPLY)) && echo ' (deleted)' || echo ' (--apply deletes them)'), ${#lkeep[@]} kept"
}

shells() {
  local min=$((HOURS * 3600))
  ps -axo pid=,ppid=,etime=,command= | perl -e '
    my $min = shift;
    sub secs { my $e = shift; my $d = $e =~ s/^(\d+)-// ? $1 : 0; my @p = split /:/, $e; unshift @p, 0 while @p < 3;
      return (($d * 24 + $p[0]) * 60 + $p[1]) * 60 + $p[2] }
    sub age { my $s = shift; $s >= 86400 ? sprintf("%dd%02dh", $s / 86400, $s % 86400 / 3600) : sprintf("%dh%02dm", $s / 3600, $s % 3600 / 60) }
    my (%p, %kids);
    while (<STDIN>) {
      my ($pid, $ppid, $et, $cmd) = /^\s*(\d+)\s+(\d+)\s+(\S+)\s+(.*?)\s*$/ or next;
      $p{$pid} = { pid => $pid, age => secs($et), cmd => $cmd };
      push @{ $kids{$ppid} }, $pid;
    }
    my (@stale, @kill, %cwd, $cur);
    for (`lsof -a -d cwd -u $< -Fpn 2>/dev/null`) { chomp; if (/^p(\d+)/) { $cur = $1 } elsif (/^n(.*)/) { $cwd{$cur} = $1 } }
    for my $s (sort { $p{$b}{age} <=> $p{$a}{age} } keys %p) {
      my $q = $p{$s};
      next unless $q->{cmd} =~ m{^\S*\b(?:zsh|bash) -c } && $q->{cmd} =~ m{\.claude/shell-snapshots/};
      next if $q->{age} < $min;
      (my $body = $q->{cmd}) =~ s/.*?\beval \x27(.*)\x27 < \/dev\/null.*/$1/s;
      my @k = @{ $kids{$s} // [] };
      my $sleeping = !grep { $p{$_}{cmd} !~ m{^(?:/bin/)?sleep \d|^tail -n? ?\S* ?-f |^tail -f |^grep } } @k;
      my $polls = $body =~ /\b(?:until|while)\b.*\bsleep\b/s || $body =~ /\btail\b[^|]*-f\b/;
      next unless $sleeping && $polls;
      # What it waits on, where that can be checked.
      my @why;
      for my $w ($body =~ /\bkill -0 (\d+)/g) { push @why, $p{$w} ? "pid $w alive" : "pid $w is gone" }
      while ($body =~ /\bpgrep -f\s+(?:"([^"]+)"|\x27([^\x27]+)\x27|(\S+))/g) {
        my $pat = $1 // $2 // $3;
        # A watcher whose own command line contains the pattern matches
        # itself: that loop never ends.
        my @live = grep { my $c = $p{$_}{cmd}; $c =~ /\Q$pat\E/ && $c !~ m{\.claude/shell-snapshots/} } keys %p;
        push @why, @live ? "\"$pat\" running (pid $live[0])" : "\"$pat\" matches only watcher shells: never ends";
      }
      # The first file the loop condition reads.
      my ($cond) = $body =~ /\b(?:until|while)\s+(.*?);\s*do\b/s;
      for my $f (($cond // "") =~ m{(?:^|[\s"\x27(])((?:~|/[^\s"\x27;)\]|\\/]+)(?:/[^\s"\x27;)\]|\\/]+)+)}g) {
        next if $f =~ m{^/dev/};
        $f =~ s{^~}{$ENV{HOME}};
        my @st = stat $f;
        push @why, !@st ? "$f missing" : sprintf("%s last written %s ago", $f =~ s{.*/}{}r, age(time - $st[9]));
        last;
      }
      push @stale, sprintf("%-7d %-7s %s%s", $s, age($q->{age}), length $body > 200 ? substr($body, 0, 200) . "..." : $body,
        (@why ? "\n                waits on: " . join("; ", @why) : "")
        . (defined $cwd{$s} ? "\n                in: $cwd{$s}" : ""));
      # Not its sleep (a new pid each round; it ends by itself), but a
      # tail -f or grep it started would outlive it.
      push @kill, $s, grep { $p{$_}{cmd} !~ m{^(?:/bin/)?sleep } } @k;
    }
    print "== stale polling shells (pid, age, what it waits for)\n";
    print "$_\n" for @stale;
    printf "%d polling shells older than %s\n", scalar @stale, age($min);
    my @mux = grep { $p{$_}{cmd} =~ /^ssh: .*\[stopped mux\]/ } sort { $a <=> $b } keys %p;
    if (@mux) {
      print "== stopped ssh multiplexers (they can hold build-host sessions open)\n";
      printf "%-7d %-7s %s\n", $_, age($p{$_}{age}), $p{$_}{cmd} for @mux;
      push @kill, @mux;
    }
    print "stop them (needs your OK):\n  kill @kill\n" if @kill;
  ' "$min"
}

case "$cmd" in
  worktrees) worktrees ;;
  branches) branches ;;
  shells) shells ;;
  all) worktrees; echo; branches; echo; shells ;;
  -h | --help) sed -n '2,/^set -euo/p' "$0" | sed '$d; s/^# \{0,1\}//' ;;
  *) echo "usage: $0 [worktrees|branches|shells|all] [--apply] [--include-locked] [--idle HOURS] [--hours N] [--no-fetch]" >&2; exit 2 ;;
esac
