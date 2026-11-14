# `scripts/offload.sh reap HOST`, this machine's side: reads the host
# runner's report on stdin (`found JSON` per offload job group, `logged
# JSON` per reap a scheduled run did earlier, `note`/`summary` lines),
# prints a table, appends one job-log line per reap to LOG, and prints one
# `event JSON` line per reap (a job.finished that ends the job in the event
# stream) for offload.sh to send.
use strict;
use warnings;
use JSON::PP;

my ($log) = @ARGV;
my $json = JSON::PP->new->canonical;
my (@rows, @done, @notes);
while (<STDIN>) {
  chomp;
  if (/^(found|logged) (\{.*\})$/) {
    my ($what, $o) = ($1, eval { decode_json($2) });
    next unless $o;
    if ($what eq 'found') { push @rows, $o; push @done, $o if $o->{action} eq 'reaped' }
    else { $o->{scheduled} = 1; push @done, $o }
  } elsif (/^(?:note|summary) (.*)$/) {
    push @notes, "offload: $1\n";
  } elsif (length) {
    print "$_\n";
  }
}

sub age {
  my $s = shift // 0;
  return sprintf('%dd%02dh', $s / 86400, $s % 86400 / 3600) if $s >= 86400;
  return sprintf('%dh%02dm', $s / 3600, $s % 3600 / 60) if $s >= 3600;
  return sprintf('%dm%02ds', $s / 60, $s % 60);
}

for my $o (@rows) {
  my $cmd = $o->{command} // '';
  $cmd = substr($cmd, 0, 300) . '...' if length $cmd > 300;
  printf "%-12s pgid %-8d %-6s %-7s %3d procs %6.1f%% cpu  %s%s\n    why: %s\n    cmd: %s\n",
    $o->{action}, $o->{pgid}, $o->{kind}, age($o->{age_s}), $o->{procs}, $o->{cpu},
    length($o->{label} // '') ? $o->{label} : '?', defined $o->{slot} ? " (slot $o->{slot})" : '',
    $o->{why}, $cmd;
}
print for @notes;
for my $o (@done) {
  print "offload: forwarded a scheduled reap: pgid $o->{pgid} ($o->{label}) at " . localtime($o->{at}) . "\n" if $o->{scheduled};
}

exit 0 unless @done;
open my $f, '>>', $log or die "offload: cannot write $log: $!\n";
for my $o (@done) {
  my $label = $o->{label} // '';
  # Slot jobs carry the client's job id; older or pre-slots jobs get one
  # named after where they ran.
  my $id = $o->{job} // "offload-reaped-$o->{host}-$o->{pgid}-$label";
  print $f $json->encode({ kind => 'reap', end => $o->{at} + 0, label => $label, host => $o->{host},
    slot => $o->{slot} // '', pgid => $o->{pgid} + 0, run_s => $o->{age_s} + 0, cpu => $o->{cpu} + 0,
    reason => $o->{why}, command => $o->{command}, job => $id,
    scheduled => $o->{scheduled} ? JSON::PP::true : JSON::PP::false }), "\n";
  print 'event ', $json->encode({ kind => 'job.finished', project => 'stave', id => $id, title => $label,
    host => $o->{host}, slot => $o->{slot} // '', outcome => 'failed', at => $o->{at} * 1000,
    detail => "reaped (lost): $o->{why}" }), "\n";
}
