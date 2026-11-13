# `scripts/offload.sh stats [HOURS]`: summarise the offload job log over the
# last HOURS (default 24) — per host and class: jobs, slot utilisation
# (slot-seconds busy over slot-seconds available), queue wait and run time
# percentiles, failures and timeouts. What slot counts and
# how many parallel agents the hosts can carry is read off this.
use strict;
use warnings;
use JSON::PP;

my ($hours, $log) = @ARGV;
$hours = 24 unless defined $hours && $hours =~ /^\d+(\.\d+)?$/ && $hours > 0;
my $since = time - $hours * 3600;
my %g;
open my $f, '<', $log or die "offload: cannot read $log: $!\n";
while (<$f>) {
  my $j = eval { decode_json($_) } or next;
  next if ($j->{end} // 0) < $since;
  my $k = "$j->{host}\t$j->{class}";
  my $s = $g{$k} //= { slots => 0, wait => [], run => [], n => 0, busy => 0, fail => 0, timeout => 0 };
  $s->{n}++;
  $s->{slots} = $j->{slots} if ($j->{slots} // 0) > $s->{slots};
  push @{ $s->{wait} }, $j->{wait_s};
  push @{ $s->{run} }, $j->{run_s};
  $s->{busy} += $j->{sync_s} + $j->{run_s};
  $s->{fail}++ if $j->{exit};
  $s->{timeout}++ if $j->{timed_out};
}
sub pct {
  my ($v, $p) = @_;
  my @s = sort { $a <=> $b } @$v;
  return 0 unless @s;
  return $s[int($p * $#s + 0.5)];
}
sub dur { my $s = shift; return $s >= 3600 ? sprintf('%.1fh', $s / 3600) : $s >= 60 ? sprintf('%.1fm', $s / 60) : "${s}s" }
printf "offload jobs, last %sh\n", $hours;
printf "%-16s %-6s %5s %6s %8s %8s %8s %8s %5s %5s\n",
  qw(host class jobs util wait-p50 wait-p95 run-p50 run-p95 fail tmout);
for my $k (sort keys %g) {
  my ($host, $class) = split /\t/, $k;
  my $s = $g{$k};
  my $cap = ($class eq 'long' ? 1 : $s->{slots} || 1) * $hours * 3600;
  printf "%-16s %-6s %5d %5.0f%% %8s %8s %8s %8s %5d %5d\n", $host, $class, $s->{n}, 100 * $s->{busy} / $cap,
    dur(pct($s->{wait}, 0.5)), dur(pct($s->{wait}, 0.95)), dur(pct($s->{run}, 0.5)), dur(pct($s->{run}, 0.95)),
    $s->{fail}, $s->{timeout};
}
print "no jobs in that window\n" unless %g;
