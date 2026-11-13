# The host side of scripts/offload.sh. Never run directly: offload.sh ships
# this file to the host on each call (base64, through `perl -e`), so a host
# needs nothing installed beyond the perl every Linux and macOS ships. Every
# argument after the mode arrives base64-encoded with a leading "x" (so an
# empty value survives the remote shell).
#
#   probe ROOT BUILD LONG CLASS             free slots, cores and load per core
#   hold  ROOT BUILD LONG CLASS HOSTNAME LABEL
#                                           take a free slot (or say 'busy'); hold it
#                                           until stdin ends
#   sync  SLOTDIR                           update SLOTDIR/tree from a tarball on stdin
#   exec  ROOT SLOT DIR TIMEOUT JOBS NICE HOSTNAME RUN FETCH
#                                           run RUN as its own session
#   gc    DIR DRYRUN                        remove legacy per-name workspaces
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
  my ($root, $want, $long, $class, $hostname, $label) = @arg;
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
  unlink map { "$root/$id.$_" } qw(owner pgid runner result release);
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

die "offload-runner: unknown mode $mode\n";
