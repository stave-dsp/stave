# The host side of scripts/offload.sh. Never run directly: offload.sh ships
# this file to the host on each call (base64, through `perl -e`), so a host
# needs nothing installed beyond the perl every Linux and macOS ships. Every
# argument after the mode arrives base64-encoded with a leading "x" (so an
# empty value survives the remote shell).
#
#   probe ROOT BUILD LONG CLASS             free slots, cores and load per core
#   hold  ROOT BUILD LONG CLASS HOSTNAME LABEL JOBID
#                                           take a free slot (or say 'busy'); hold it
#                                           until stdin ends
#   sync  SLOTDIR                           update SLOTDIR/tree from a tarball on stdin
#   exec  ROOT SLOT DIR TIMEOUT JOBS NICE HOSTNAME RUN FETCH
#                                           run RUN as its own session
#   gc    DIR DRYRUN                        remove legacy per-name workspaces
#   reap  DIR DRYRUN LEGACY PGIDS GRACE HOSTNAME FORWARD
#                                           stop offload jobs nobody owns any more
#   install-reaper DIR EVERY RUNNER ARGS... schedule `reap` in this account
#
# Slots: ROOT/<id>.lock is a flock; the build slots are 1..BUILD, the long
# slot (when LONG is 1) is L1; ROOT/long.lock admits one `long` job per host.
# A slot's workspace is ROOT/<id>/tree (the build cache in tree/target).
#
# Stopping a job. A host may refuse signals between processes outright (a
# locked-down container: kill(2) fails with EACCES even on one's own child,
# which is why `timeout` and `kill` left 14-hour orphans). So the command
# runs as a session leader with a pseudo-terminal as its controlling
# terminal, the master end held only here: closing it is a hangup, and the
# kernel itself delivers SIGHUP to the session leader and, as it exits, to
# the whole foreground process group — no kill(2) needed. Where kill(2) is
# allowed, the group also gets TERM, then KILL.
use strict;
use warnings;
use Fcntl qw(:flock O_RDWR O_CREAT O_NOCTTY);
use POSIX qw(setsid WNOHANG);
use Errno qw(ESRCH);
use IO::Select;
use File::Path qw(make_path remove_tree);
use File::Find;
use File::Basename qw(dirname);
use Cwd ();
use MIME::Base64;
use JSON::PP;
$| = 1;

my $mode = shift @ARGV;
my @arg = map { substr(decode_base64($_), 1) } @ARGV;

sub cores {
  for my $c ('nproc 2>/dev/null', 'sysctl -n hw.ncpu 2>/dev/null') {
    my $n = `$c`;
    return $1 if defined $n && $n =~ /(\d+)/ && $1 > 0;
  }
  return 1;
}

sub load1 {
  if (open my $f, '<', '/proc/loadavg') { my $l = <$f>; return (split ' ', $l)[0] }
  my $s = `sysctl -n vm.loadavg 2>/dev/null`;
  return $s =~ /([\d.]+)/ ? $1 : 0;
}

sub home_path { my $p = shift; return $p =~ m{^/} ? $p : "$ENV{HOME}/$p" }

# Build slots: as asked, else a quarter of the cores (6 → 2, 8 → 2).
sub build_slots {
  my ($want, $cores) = @_;
  return int($want) if defined $want && $want =~ /^\d+$/ && $want > 0;
  my $s = int(($cores + 3) / 4);
  return $s < 1 ? 1 : $s;
}

sub try_lock {
  my ($path) = @_;
  sysopen(my $fh, $path, O_RDWR | O_CREAT) or die "offload: cannot open $path: $!\n";
  return flock($fh, LOCK_EX | LOCK_NB) ? $fh : undef;
}

sub slurp { my $p = shift; open my $f, '<', $p or return ''; local $/; my $s = <$f>; $s =~ s/\s+$//; return $s }

sub spew { my ($p, $s) = @_; open my $f, '>', $p or return; print $f $s }

# A process (or, negative, a process group) exists — even one we may not
# signal (EPERM / EACCES both mean it is there).
sub alive { my $p = shift; return 0 unless $p; return 1 if kill 0, $p; return $! != ESRCH }

# The slot ids a job of CLASS may take, in order, and the gate it also
# needs (`long` jobs: one per host).
sub candidates {
  my ($class, $build, $long) = @_;
  return ([1 .. $build], undef) if $class ne 'long';
  return (['L1'], 'long') if $long;
  return ([1 .. $build], 'long');
}

sub owners {
  my ($root, $ids) = @_;
  return join ', ', map { my $o = slurp("$root/$_.owner"); "$_: " . ($o eq '' ? '?' : $o) } @$ids;
}

if ($mode eq 'probe') {
  my ($root, $want, $long, $class) = @arg;
  $root = home_path($root);
  make_path($root);
  my $cores = cores();
  my $build = build_slots($want, $cores);
  my ($ids, $gate) = candidates($class, $build, $long);
  my $free = 0;
  if (!$gate || try_lock("$root/$gate.lock")) {
    for my $id (@$ids) { $free++ if try_lock("$root/$id.lock") }
  }
  printf "free %d slots %d cores %d load %.3f\n", $free, $build, $cores, load1() / $cores;
  exit 0;
}

if ($mode eq 'hold') {
  my ($root, $want, $long, $class, $hostname, $label, $jobid) = @arg;
  $root = home_path($root);
  make_path($root);
  my $ppid = getppid();
  my $cores = cores();
  my $build = build_slots($want, $cores);
  my ($ids, $gate) = candidates($class, $build, $long);
  my ($gate_fh, $fh, $id);
  # Busy: say who holds what. offload.sh queues on this side (an ssh
  # session per waiting job would exhaust the host's sshd MaxSessions).
  $gate_fh = $gate ? try_lock("$root/$gate.lock") : 1;
  if ($gate_fh) {
    for my $c (@$ids) { if ($fh = try_lock("$root/$c.lock")) { $id = $c; last } }
  }
  if (!$fh) {
    my $on_gate = !$gate_fh;
    my $what = $on_gate ? "the one $class slot" : "all " . scalar(@$ids) . " $class slots";
    print "busy $what on $hostname busy (" . owners($root, $on_gate ? [$gate] : $ids) . ")\n";
    exit 75;
  }
  my $own = "$label (since " . scalar(localtime) . ")";
  spew("$root/$id.owner", $own);
  spew("$root/$gate.owner", "$own in slot $id") if $gate;
  unlink map { "$root/$id.$_" } qw(pgid runner result release);
  # The job's id in the client's event stream, so a reap of this slot can
  # finish the same job.
  spew("$root/$id.job", $jobid // '');
  make_path("$root/$id/tree");
  my $jobs = int($cores / $build) || 1;
  print "slot $id slots $build cores $cores jobs $jobs\n";
  my $sel = IO::Select->new(\*STDIN);
  # Hold until offload.sh closes our stdin (done, or it died) or the ssh
  # session that started us is gone; then the job (if it still runs) is
  # told to stop, and the slot is free once it has.
  while (1) {
    if ($sel->can_read(2)) { my $n = sysread(STDIN, my $buf, 4096); last if !$n }
    last if getppid() != $ppid;
  }
  my $runner = slurp("$root/$id.runner");
  if ($runner =~ /^\d+$/ && alive($runner)) {
    spew("$root/$id.release", time);
    my $t = time;
    select(undef, undef, undef, 0.2) while alive($runner) && time - $t < 120;
  }
  my $result = slurp("$root/$id.result");
  print "result $result\n" if $result ne '';
  unlink map { "$root/$id.$_" } qw(owner pgid runner result release job);
  unlink "$root/$gate.owner" if $gate;
  exit 0;
}

# Tar hosts: the tarball (exactly the files git tracks or would track)
# arrives on stdin; the slot's tree takes it by content, so a file whose
# bytes did not change keeps its mtime and cargo's incremental state stays
# valid, and a changed file is stamped now so cargo rebuilds it. Then the
# tree's git index is exactly those files and anything else not ignored is
# cleaned away (build output, being ignored, stays). No rsync here: rsync
# signals its own children, which such hosts refuse.
if ($mode eq 'sync') {
  my ($slot) = @arg;
  $slot = home_path($slot);
  my ($in, $tree) = ("$slot/incoming", "$slot/tree");
  remove_tree($in);
  make_path($in, $tree);
  system('tar', '-xzf', '-', '-C', $in) == 0 or die "offload: unpacking the tree failed\n";
  my @files;
  find({ no_chdir => 1, wanted => sub {
    return if -d $_ && !-l $_;
    push @files, substr($File::Find::name, length($in) + 1);
  } }, $in);
  my ($same, $changed) = (0, 0);
  for my $f (@files) {
    my ($src, $dst) = ("$in/$f", "$tree/$f");
    if (-l $src) {
      if (-l $dst && readlink($dst) eq readlink($src)) { $same++; next }
    } elsif (-f $dst && !-l $dst && -s $dst == -s $src && (stat $dst)[2] == (stat $src)[2]) {
      my $eq = 1;
      open my $a, '<:raw', $src or die; open my $b, '<:raw', $dst or die;
      while (1) {
        my $na = read $a, my $ba, 1 << 16;
        read $b, my $bb, 1 << 16;
        if ($ba ne $bb) { $eq = 0; last }
        last unless $na;
      }
      if ($eq) { $same++; next }
    }
    make_path(dirname($dst));
    remove_tree($dst) if -d $dst && !-l $dst;
    rename $src, $dst or die "offload: cannot place $f: $!\n";
    utime undef, undef, $dst unless -l $dst;
    $changed++;
  }
  remove_tree($in);
  chdir $tree or die;
  my $git = sub { system('git', @_) == 0 or die "offload: git @_ failed in the slot\n" };
  $git->('init', '-q') unless -d '.git';
  $git->('read-tree', '--empty');
  open my $list, '|-', 'git', 'update-index', '--add', '-z', '--stdin' or die;
  print $list join("\0", @files), "\0";
  close $list or die "offload: indexing the tree failed\n";
  $git->('clean', '-fdq');
  print STDERR "offload: slot tree: $changed files updated, $same unchanged\n";
  exit 0;
}

# Linux: a pseudo-terminal pair without IO::Pty (TIOCSPTLCK, TIOCGPTN).
sub open_pty {
  return unless $^O eq 'linux' && -e '/dev/ptmx';
  sysopen(my $m, '/dev/ptmx', O_RDWR | O_NOCTTY) or return;
  my $unlock = pack('i', 0);
  ioctl($m, 0x40045431, $unlock) or return;
  my $n = pack('I', 0);
  ioctl($m, 0x80045430, $n) or return;
  return ($m, '/dev/pts/' . unpack('I', $n));
}

if ($mode eq 'exec') {
  my ($root, $id, $dir, $timeout, $jobs, $nice, $hostname, $run, $fetch) = @arg;
  $root = home_path($root);
  $dir = home_path($dir);
  chdir $dir or die "offload: cannot enter $dir: $!\n";
  # Stale outputs from the slot's last job must not be fetched back as ours.
  for my $p (split ' ', $fetch) {
    $p =~ s{/+$}{};
    next if $p eq '' || $p =~ m{^/} || $p =~ m{(^|/)\.\.(/|$)};
    remove_tree($p) if -e $p || -l $p;
  }
  # One slot's share of the host: every parallel knob the workspace's tools
  # read (cargo, nextest, libtest, rayon, OpenMP — ngspice).
  for my $k (qw(CARGO_BUILD_JOBS NEXTEST_TEST_THREADS RUST_TEST_THREADS RAYON_NUM_THREADS OMP_NUM_THREADS)) {
    $ENV{$k} = $jobs unless defined $ENV{$k} && $ENV{$k} ne '';
  }
  $ENV{STAVE_OFFLOAD_JOBS} = $jobs;
  $ENV{STAVE_OFFLOAD_SLOT} = $id;
  my $secs = 0;
  if ($timeout ne '') {
    my %unit = (s => 1, m => 60, h => 3600, d => 86400);
    $timeout =~ /^(\d+)([smhd]?)$/ or die "offload: STAVE_OFFLOAD_TIMEOUT=$timeout: want seconds, or a number with s, m, h or d\n";
    $secs = $1 * $unit{$2 || 's'};
  }
  my $ppid = getppid();
  my ($master, $slave) = open_pty();
  my $pid = fork() // die "offload: fork: $!\n";
  if (!$pid) {
    setsid();
    if ($slave) {
      close $master;
      # Opening the terminal makes it this session's controlling terminal.
      if (sysopen(my $tty, $slave, O_RDWR)) { close $tty }
    }
    exec 'nice', '-n', $nice, 'bash', '-c', $run;
    die "offload: exec: $!\n";
  }
  spew("$root/$id.pgid", $pid);
  spew("$root/$id.runner", $$);
  my $stop = '';
  $SIG{$_} = sub { $stop = "got SIG$_[0]" } for qw(TERM INT HUP);
  $SIG{PIPE} = 'IGNORE';
  my $deadline = $secs ? time + $secs : 0;
  my ($status, $timed_out);
  # Stop the job: hang up its terminal (and TERM its group where signals
  # are allowed); after GRACE seconds, KILL where allowed. Returns what is
  # left of the group.
  my $reap = sub {
    $status = $? & 127 ? 128 + ($? & 127) : $? >> 8 if !defined $status && waitpid($pid, WNOHANG) == $pid;
  };
  my $settle = sub {
    my $t = time;
    while (time - $t < $_[0]) {
      $reap->();
      return if defined $status && !alive(-$pid);
      select(undef, undef, undef, 0.2);
    }
  };
  my $halt = sub {
    my ($grace) = @_;
    kill 'TERM', -$pid;
    undef $master;
    $settle->($grace);
    if (alive(-$pid)) { kill 'KILL', -$pid; $settle->(2) }
    $reap->();
    return alive(-$pid);
  };
  while (1) {
    if (waitpid($pid, WNOHANG) == $pid) {
      $status = $? & 127 ? 128 + ($? & 127) : $? >> 8;
      last;
    }
    $stop ||= 'the ssh session ended' if getppid() != $ppid;
    $stop ||= 'offload.sh went away' if -e "$root/$id.release";
    if ($stop) {
      print STDERR "offload: $stop; stopping the job in slot $id on $hostname\n";
      my $left = $halt->(10);
      print STDERR "offload: the job in slot $id on $hostname would not stop; processes remain\n" if $left;
      $status //= 143;
      last;
    }
    if ($deadline && time >= $deadline) {
      print STDERR "\noffload: TIMEOUT: STAVE_OFFLOAD_TIMEOUT=$timeout fired after ${secs}s in slot $id on $hostname; "
        . "stopping the job (hangup and TERM, KILL after 30s)\n";
      my $left = $halt->(30);
      print STDERR "offload: the job in slot $id on $hostname would not stop; processes remain\n" if $left;
      ($status, $timed_out) = (124, 1);
      last;
    }
    select(undef, undef, undef, 0.25);
  }
  # The leader is gone; the kernel hangs up its foreground group as it
  # exits. Anything the command left running in it goes too.
  $halt->(5) if alive(-$pid);
  my $result = "exit $status timed_out " . ($timed_out ? 1 : 0);
  spew("$root/$id.result", $result);
  unlink "$root/$id.pgid", "$root/$id.runner";
  exit $status;
}

if ($mode eq 'gc') {
  my ($dir, $dry) = @arg;
  $dir = home_path($dir);
  # Resolved: process working directories are real paths (a host may keep
  # the directory behind a symlink).
  $dir = Cwd::abs_path($dir) // $dir;
  opendir(my $dh, $dir) or do { print "nothing at $dir\n"; exit 0 };
  my %group;
  for my $e (grep { !/^\.\.?$/ && $_ ne 'slots' } readdir $dh) {
    (my $base = $e) =~ s/\.(lock|target|git)$//;
    push @{ $group{$base} }, $e;
  }
  # Working directories of this user's processes: a legacy workspace a
  # running job sits in is kept.
  my @cwds;
  if (-d '/proc/self') {
    for my $p (glob '/proc/[0-9]*') { my $c = readlink "$p/cwd"; push @cwds, $c if defined $c }
  } else {
    for (`lsof -a -d cwd -u "\$(id -u)" -Fn 2>/dev/null`) { push @cwds, $1 if /^n(.*)/ }
  }
  my ($freed, $removed, $kept) = (0, 0, 0);
  for my $base (sort keys %group) {
    my @paths = map { "$dir/$_" } @{ $group{$base} };
    my $workspace = -f "$dir/$base/Cargo.toml" || -d "$dir/$base.target" || -d "$dir/$base.git"
      || (@paths == 1 && $paths[0] =~ /\.lock$/);
    my $why = '';
    if (!$workspace) { $why = 'not an offload workspace' }
    elsif (-e "$dir/$base.lock" && !try_lock("$dir/$base.lock")) { $why = 'its lock is held' }
    elsif (grep { my $c = $_; grep { $c eq $_ || index($c, "$_/") == 0 } @paths } @cwds) { $why = 'a process runs in it' }
    elsif (grep { -e $_ && `find '$_' -mmin -15 -print 2>/dev/null | head -1` ne '' } @paths) { $why = 'written in the last 15 minutes' }
    if ($why) { print "kept    $base: $why\n"; $kept++; next }
    my $kb = 0;
    for (`du -sk @{[map { "'$_'" } @paths]} 2>/dev/null`) { $kb += $1 if /^(\d+)/ }
    remove_tree(@paths) unless $dry;
    printf "%s $base (%.1f GB)\n", $dry ? 'would remove' : 'removed', $kb / 1048576;
    $freed += $kb;
    $removed++;
  }
  printf "%s %d legacy workspaces, %.1f GB; kept %d\n", $dry ? 'would remove' : 'removed', $removed, $freed / 1048576, $kept;
  exit 0;
}

# Elapsed time from ps (`[[dd-]hh:]mm:ss`) in seconds.
sub etime_s {
  my ($e) = @_;
  my $d = $e =~ s/^(\d+)-// ? $1 : 0;
  my @p = split /:/, $e;
  unshift @p, 0 while @p < 3;
  return (($d * 24 + $p[0]) * 60 + $p[1]) * 60 + $p[2];
}

# This user's processes: pid → {pid ppid pgid age cpu args cwd}. Only the
# calling user's processes are ever listed (in a container, ps sees only
# the container's), so a reap can never reach anyone else's.
sub own_processes {
  my %p;
  for (`ps -A -o pid= -o ppid= -o pgid= -o uid= -o etime= -o pcpu= -o args= 2>/dev/null`) {
    my ($pid, $ppid, $pgid, $uid, $et, $cpu, $args) = /^\s*(\d+)\s+(\d+)\s+(\d+)\s+(\d+)\s+(\S+)\s+(\S+)\s+(.*?)\s*$/ or next;
    next unless $uid == $<;
    $p{$pid} = { pid => $pid, ppid => $ppid, pgid => $pgid, age => etime_s($et), cpu => $cpu + 0, args => $args };
  }
  # Working directory and session where they can be read: undef when not
  # (a locked-down container may refuse another process's cwd even to its
  # owner; its session id, in /proc/PID/stat, stays readable).
  if (-d '/proc/self') {
    for my $pid (keys %p) {
      $p{$pid}{cwd} = readlink "/proc/$pid/cwd";
      my $st = slurp("/proc/$pid/stat");
      $p{$pid}{sid} = $1 if $st =~ /\)\s+\S+\s+\d+\s+\d+\s+(\d+)/;
    }
  } else {
    my $cur;
    for (`lsof -a -d cwd -u $< -Fpn 2>/dev/null`) {
      chomp;
      if (/^p(\d+)/) { $cur = $1 } elsif (/^n(.*)/ && $cur && $p{$cur}) { $p{$cur}{cwd} = $1 }
    }
  }
  return \%p;
}

# Reap: find offload jobs nobody owns any more and stop each one's whole
# process group (TERM, GRACE seconds, KILL). A job is orphaned when
#   slot    its slot's lock is free: the holder (alive exactly as long as
#           the client's ssh session) is gone, yet the job runs on;
#   stray   a process left the group of an idle slot's job but not its
#           session, or sits in an idle slot's workspace (the job's stop,
#           which signals its group, missed it);
#   legacy  a job of the pre-slots script (DIR/<name>, DIR/<name>.lock)
#           whose ssh session is gone — or any of them with LEGACY set, for
#           when no client of that script remains.
# PGIDS (space-separated) restricts the reap to those groups (and reaps them
# even if a client still seems connected). Every reap is appended to
# DIR/reaped.jsonl; with FORWARD, entries not yet shown to a client are
# printed first (`logged`), so a scheduled run's reaps reach the client's
# job log and event stream.
if ($mode eq 'reap') {
  my ($dir, $dry, $legacy, $pgids, $grace, $hostname, $forward) = @arg;
  # As command lines spell it, and resolved (as working directories are:
  # a host may keep the directory behind a symlink).
  my $named = home_path($dir);
  (my $base = $named) =~ s{.*/}{};
  $dir = Cwd::abs_path($named) // $named;
  my $slots = "$dir/slots";
  my $home = Cwd::abs_path($ENV{HOME}) // $ENV{HOME};
  $grace = 10 unless defined $grace && $grace =~ /^\d+$/;
  my %want = map { $_ => 1 } grep { /^\d+$/ } split /[\s,]+/, $pgids // '';
  my $json = JSON::PP->new->canonical;
  my $log = "$dir/reaped.jsonl";
  if ($forward && !$dry && -f $log) {
    my $off = slurp("$dir/reaped.forwarded") || 0;
    if (open my $f, '<', $log) {
      seek $f, $off, 0 if $off <= -s $log;
      while (<$f>) { chomp; print "logged $_\n" if length }
    }
  }
  my $procs = own_processes();
  # Never ourselves, nor the session (ssh, shell) that started us.
  my %spare;
  for (my $p = $$; $p && $procs->{$p}; $p = $procs->{$p}{ppid}) {
    $spare{ $procs->{$p}{pgid} } = 1;
    last if $p == $procs->{$p}{ppid};
  }
  my %group;
  push @{ $group{ $_->{pgid} } }, $_ for values %$procs;
  # Each slot's recorded job: its group, its runner, who took it.
  my (%slot_of_pgid, %slot_of_runner, %slotinfo);
  if (opendir my $sd, $slots) {
    for my $f (readdir $sd) {
      my ($id) = $f =~ /^(\w+)\.lock$/ or next;
      next if $id eq 'long';
      my %s = (pgid => slurp("$slots/$id.pgid"), runner => slurp("$slots/$id.runner"),
        owner => slurp("$slots/$id.owner"), job => slurp("$slots/$id.job"));
      $slotinfo{$id} = \%s;
      $slot_of_pgid{ $s{pgid} } = $id if $s{pgid} =~ /^\d+$/;
      $slot_of_runner{ $s{runner} } = $id if $s{runner} =~ /^\d+$/;
    }
  }
  # A free slot's lock stays taken until we exit, so no job starts there
  # while we reap it.
  my %taken;
  my $is_free = sub {
    my $id = shift;
    return 1 if $taken{$id};
    my $fh = try_lock("$slots/$id.lock") or return 0;
    $taken{$id} = $fh;
    return 1;
  };
  my @found;
  for my $pg (sort { $a <=> $b } keys %group) {
    next if $spare{$pg};
    my @ps = @{ $group{$pg} };
    my ($slot, $name, $kind, $why, $reap);
    $slot = $slot_of_pgid{$pg} if exists $slot_of_pgid{$pg};
    for my $pr (@ps) { $slot //= $slot_of_runner{ $pr->{pid} } if exists $slot_of_runner{ $pr->{pid} } }
    if (defined $slot) {
      $kind = 'slot';
    } else {
      # Left the job's group but not its session (the runner makes the job
      # a session leader), or sits in a slot's workspace.
      for my $pr (@ps) {
        if (defined $pr->{sid} && exists $slot_of_pgid{ $pr->{sid} }) { ($slot, $kind) = ($slot_of_pgid{ $pr->{sid} }, 'stray'); last }
        if (($pr->{cwd} // '') =~ m{^\Q$slots\E/(\w+)(/|$)}) { ($slot, $kind) = ($1, 'stray'); last }
      }
    }
    if (!$kind) {
      my $legacy_path = qr{([A-Za-z0-9._-]+?)(?:\.lock|\.target|\.git)?(?:['"/\s]|$)};
      for my $pr (@ps) {
        my $cwd = $pr->{cwd};
        if (($cwd // '') =~ m{^\Q$dir\E/([^/]+?)(?:\.target|\.git)?(?:/|$)} && $1 ne 'slots' && $1 ne 'bin') { $name = $1; last }
        # The pre-slots script named its paths absolutely or relative to
        # the home directory, where its commands started (relative ones
        # count from anywhere else only when the cwd cannot be read).
        if ($pr->{args} =~ m{(?:\Q$dir\E|\Q$named\E)/$legacy_path} && $1 ne 'slots' && $1 ne 'bin') { $name = $1; last }
        if ((!defined $cwd || $cwd eq $home)
          && $pr->{args} =~ m{(?:^|[\s'"=])\Q$base\E/$legacy_path} && $1 ne 'slots' && $1 ne 'bin') { $name = $1; last }
      }
      $kind = 'legacy' if defined $name;
    }
    next unless $kind;
    my $leader = $procs->{$pg} // (sort { $b->{age} <=> $a->{age} } @ps)[0];
    my $cpu = 0;
    $cpu += $_->{cpu} for @ps;
    my %o = (pgid => $pg + 0, kind => $kind, host => $hostname, age_s => $leader->{age},
      cpu => sprintf('%.1f', $cpu) + 0, procs => scalar @ps,
      command => $leader->{args} =~ /decode_base64\(shift\)/ ? '(the offload runner)' : $leader->{args});
    if ($kind ne 'legacy') {
      my $s = $slotinfo{$slot} // {};
      $o{slot} = $slot;
      ($o{label} = $s->{owner} // '') =~ s/ \(since .*\)$//;
      $o{job} = $s->{job} if length($s->{job} // '');
      if ($is_free->($slot)) {
        $why = $kind eq 'slot' ? "slot ${slot}'s holder is gone (its lock is free)" : "outside its job's process group; slot $slot is idle";
        $reap = 1;
      } else {
        $why = $kind eq 'slot' ? "slot $slot is held: its client is connected" : "slot $slot is busy: may belong to its job";
      }
    } else {
      $o{label} = $name;
      # The pre-slots script ran the job under the ssh session that sent it.
      my $sshd;
      for (my $p = $pg; $p && $procs->{$p} && $p != 1; $p = $procs->{$p}{ppid}) {
        if ($procs->{$p}{args} =~ /^sshd/) { $sshd = $procs->{$p}; last }
        last if $p == $procs->{$p}{ppid};
      }
      if (!$sshd) { ($why, $reap) = ('pre-slots job; its ssh session is gone', 1) }
      elsif ($legacy) { ($why, $reap) = ("pre-slots job (--legacy); its ssh session $sshd->{pid} is $sshd->{age}s old", 1) }
      else { $why = "pre-slots job; its ssh session $sshd->{pid} ($sshd->{age}s old) is open: --legacy or --pgid $pg reaps it" }
    }
    if (%want) {
      if ($want{$pg}) {
        ($why =~ s/: --legacy or --pgid \d+ reaps it$//, $why .= '; selected by --pgid') unless $reap;
        $reap = 1;
      }
      else { ($why, $reap) = ("$why [not selected]", 0) }
    }
    $o{why} = $why;
    $o{action} = !$reap ? 'kept' : $dry ? 'would reap' : 'reap';
    push @found, \%o;
  }
  for my $pg (sort keys %want) {
    print "note pgid $pg is not an offload job of this user here; left alone\n" unless grep { $_->{pgid} == $pg } @found;
  }
  for my $o (@found) {
    if ($o->{action} eq 'reap') {
      my $pg = $o->{pgid};
      my $sent = kill 'TERM', -$pg;
      my $err = $sent ? '' : "$!";
      my $t = time;
      select(undef, undef, undef, 0.2) while alive(-$pg) && time - $t < $grace;
      if (alive(-$pg)) {
        kill 'KILL', -$pg;
        $t = time;
        select(undef, undef, undef, 0.2) while alive(-$pg) && time - $t < 5;
      }
      if (alive(-$pg)) {
        $o->{action} = $sent ? 'survived TERM and KILL' : "refused: $err";
      } else {
        $o->{action} = 'reaped';
        $o->{at} = time;
        unlink map { "$slots/$o->{slot}.$_" } qw(owner pgid runner result release job) if $o->{kind} eq 'slot';
        if (open my $f, '>>', $log) { print $f $json->encode($o), "\n" }
      }
    }
    print "found ", $json->encode($o), "\n";
  }
  spew("$dir/reaped.forwarded", -s $log) if $forward && !$dry && -f $log;
  printf "summary %d offload job groups: %d %s, %d kept\n", scalar @found,
    scalar(grep { $_->{action} =~ /reap/ } @found), $dry ? 'to reap' : 'reaped',
    scalar(grep { $_->{action} eq 'kept' } @found);
  exit 0;
}

# Schedule `reap` in this account: a systemd user timer (Linux) or a launchd
# agent (macOS), every EVERY minutes (0 removes it). RUNNER is this file
# (base64), installed as DIR/bin/offload-runner.pl beside a DIR/bin/reap
# wrapper that holds reap's arguments (ARGS, already encoded).
if ($mode eq 'install-reaper') {
  my ($dir, $every, $runner, @reap) = @arg;
  $dir = home_path($dir);
  $every =~ /^\d+$/ or die "offload: --every takes whole minutes\n";
  my ($bin, $wrap) = ("$dir/bin/offload-runner.pl", "$dir/bin/reap");
  my $say = sub { my $r = system(@_); print "ran @_: ", ($r == 0 ? 'ok' : 'exit ' . ($? >> 8)), "\n"; $r == 0 };
  my $install = sub {
    make_path("$dir/bin");
    spew($bin, decode_base64($runner));
    spew($wrap, "#!/bin/sh\n# stave offload reaper (installed by scripts/offload.sh reap HOST --install)\n"
      . "cd \"\$HOME\" || exit 1\nexec nice -n 19 perl '$bin' reap " . join(" ", map { encode_base64("x$_", "") } @reap) . "\n");
    chmod 0755, $wrap;
  };
  if ($^O eq 'linux') {
    my $u = "$ENV{HOME}/.config/systemd/user";
    my $unit = 'stave-offload-reap';
    if ($every == 0) {
      $say->('systemctl', '--user', 'disable', '--now', "$unit.timer");
      unlink "$u/$unit.service", "$u/$unit.timer", $wrap, $bin;
      $say->('systemctl', '--user', 'daemon-reload');
      print "removed the reaper\n";
      exit 0;
    }
    $install->();
    make_path($u);
    spew("$u/$unit.service", "[Unit]\nDescription=stave offload: stop jobs nobody owns any more\n\n"
      . "[Service]\nType=oneshot\nExecStart=/bin/sh $wrap\n");
    spew("$u/$unit.timer", "[Unit]\nDescription=stave offload reaper\n\n"
      . "[Timer]\nOnBootSec=5min\nOnUnitActiveSec=${every}min\n\n[Install]\nWantedBy=timers.target\n");
    $say->('systemctl', '--user', 'daemon-reload') or exit 1;
    $say->('systemctl', '--user', 'enable', '--now', "$unit.timer") or exit 1;
    print "warning: lingering is off for this user, so the timer runs only while one of its sessions is open; "
      . "an administrator of this machine (or container) can run: loginctl enable-linger " . getpwuid($<) . "\n"
      if `loginctl show-user "\$(id -un)" -p Linger 2>/dev/null` =~ /=no/;
    print "installed $unit.timer: every $every min, runs $wrap\n";
  } elsif ($^O eq 'darwin') {
    # launchd first. An account with no login session (a hidden build
    # account reached only by ssh) may not be allowed to bootstrap an agent
    # into its own domain; then the account's crontab.
    my $plist = "$ENV{HOME}/Library/LaunchAgents/dev.stave.offload-reap.plist";
    my $tag = '# stave-offload-reap';
    my $cron = sub {
      my ($line) = @_;
      my @all = `crontab -l 2>/dev/null`;
      my @keep = grep { index($_, $tag) < 0 } @all;
      return 1 if !defined $line && @keep == @all;
      open my $c, '|-', 'crontab', '-' or return 0;
      print $c @keep, defined $line ? "$line\n" : ();
      return close $c;
    };
    for my $domain ("gui/$<", "user/$<") { system("launchctl bootout $domain '$plist' >/dev/null 2>&1") if -e $plist }
    $cron->(undef);
    if ($every == 0) { unlink $plist, $wrap, $bin; print "removed the reaper\n"; exit 0 }
    $install->();
    make_path(dirname($plist));
    spew($plist, qq{<?xml version="1.0" encoding="UTF-8"?>\n}
      . qq{<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">\n}
      . qq{<plist version="1.0"><dict>\n<key>Label</key><string>dev.stave.offload-reap</string>\n}
      . qq{<key>ProgramArguments</key><array><string>/bin/sh</string><string>$wrap</string></array>\n}
      . qq{<key>StartInterval</key><integer>} . ($every * 60) . qq{</integer>\n<key>RunAtLoad</key><true/>\n}
      . qq{<key>EnvironmentVariables</key><dict><key>PATH</key><string>/usr/bin:/bin:/usr/sbin:/sbin</string></dict>\n}
      . qq{<key>StandardOutPath</key><string>$dir/reaper.out</string>\n}
      . qq{<key>StandardErrorPath</key><string>$dir/reaper.out</string>\n</dict></plist>\n});
    for my $domain ("gui/$<", "user/$<") {
      next unless system("launchctl bootstrap $domain '$plist' >/dev/null 2>&1") == 0;
      print "installed dev.stave.offload-reap: every $every min (launchd $domain), runs $wrap\n";
      exit 0;
    }
    unlink $plist;
    print "launchd refused an agent in this account (no login session); using its crontab\n";
    my $when = $every < 60 ? "*/$every * * * *" : $every % 60 == 0 && $every < 1440 ? '0 */' . ($every / 60) . ' * * *'
      : die "offload: crontab takes --every under 60 minutes, or whole hours under a day\n";
    $cron->("$when /bin/sh '$wrap' >> '$dir/reaper.out' 2>&1 $tag") or die "offload: crontab refused the entry\n";
    print "installed a crontab entry: every $every min, runs $wrap\n";
  } else {
    die "offload: no scheduler for $^O\n";
  }
  exit 0;
}

die "offload-runner: unknown mode $mode\n";
