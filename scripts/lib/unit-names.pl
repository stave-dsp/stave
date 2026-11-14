#!/usr/bin/env perl
# Units are types (DSP.md U4/U5): finds the public functions of the
# Rust files named on the command line that carry a quantity as a bare
# float, and prints one `path:line: fn name — why` line each. Read by
# scripts/check-vocabulary.sh, which owns the allow-list.
#
# A public function (`pub fn`, `pub const fn`; signatures may span
# lines) is reported when
#
#   * its name ends in a unit suffix: `_hz`, `_db`, `_ms`, `_s`,
#     `_secs`, `_seconds` (`set_t60_s`, `gain_db`): the unit belongs in
#     the type, so the name says the role (`set_t60(Seconds)`);
#   * a parameter is a bare `f32`/`f64` named for a unit (`hz`, `db`,
#     `ms`, `secs`, `seconds`), or any float-carrying parameter whose name
#     ends in one of the suffixes above (`cutoff_hz: f32`,
#     `attack_ms: f32`, `highpass_hz: Option<f32>`);
#   * it is a setter of a quantity that always has a unit — `set_cutoff`,
#     `set_frequency`, `set_pitch`, `set_corner`, `set_t60`, `set_decay`,
#     `set_attack`, `set_release`, `set_delay`, `set_predelay`,
#     `set_lookahead`, `set_ceiling`, `set_makeup`, `set_threshold` (alone
#     or as a prefix: `set_attack_time`) — and takes a bare `f32`/`f64`.
#
#   perl scripts/lib/unit-names.pl FILE...
use strict;
use warnings;

my $UNIT = qr/(?:hz|db|ms|s|secs|seconds)/;
# A parameter named for its unit alone (`hz: f32`); a lone `s` is too
# common a name (a sample, a scale) to mean seconds.
my $BARE = qr/(?:hz|db|ms|secs|seconds)/;
my $SETTER = qr/set_(?:cutoff|frequency|pitch|corner|t60|decay|attack|release|delay|predelay|lookahead|ceiling|makeup|threshold)(?:_\w+)?/;
# Balanced parentheses: a closure type (`impl FnMut(f64) -> f64`) may sit
# inside a parameter list.
my $PARENS;
$PARENS = qr/\((?:[^()]++|(??{$PARENS}))*\)/;

for my $path (@ARGV) {
    open my $fh, '<', $path or die "$path: $!";
    local $/;
    my $src = <$fh>;
    close $fh;
    while ($src =~ /\bpub(?:\s+const)?(?:\s+unsafe)?\s+fn\s+(\w+)\s*(?:<[^(]*?>)?\s*($PARENS)/g) {
        my ($name, $args) = ($1, $2);
        my $line = 1 + (() = substr($src, 0, $-[0]) =~ /\n/g);
        my @why;
        push @why, "the name carries the unit" if $name =~ /_$UNIT$/;
        # Parameters: split the list at top-level commas.
        my $inner = substr($args, 1, -1);
        $inner =~ s/->/=>/g; # an arrow is not a closing bracket
        my @params;
        my ($depth, $cur) = (0, '');
        for my $c (split //, $inner) {
            $depth++ if $c =~ /[(<\[]/;
            $depth-- if $c =~ /[)>\]]/;
            if ($c eq ',' && $depth == 0) { push @params, $cur; $cur = ''; next }
            $cur .= $c;
        }
        push @params, $cur;
        my $floats = 0;
        for my $p (@params) {
            next unless $p =~ /^\s*(?:mut\s+)?(\w+)\s*:\s*(.*?)\s*$/s;
            my ($arg, $ty) = ($1, $2);
            next unless $ty =~ /\bf(?:32|64)\b/;
            $floats++ if $ty =~ /^f(?:32|64)$/;
            if ($ty =~ /^f(?:32|64)$/ && $arg =~ /^$BARE$/) {
                push @why, "`$arg: $ty` is a bare unit";
            } elsif ($arg =~ /_$UNIT$/) {
                push @why, "`$arg: $ty` carries its unit in the name";
            }
        }
        push @why, "a setter of a unit quantity takes a bare float" if $floats && $name =~ /^$SETTER$/;
        print "$path:$line: fn $name — ", join('; ', @why), "\n" if @why;
    }
}
