#!/usr/bin/env perl
# Every Kani proof harness in the Rust files named on the command line
# (paths `crates/<crate>/…`), one `crate harness` line each, sorted: the
# function each `#[kani::proof]` attribute marks. Read by scripts/kani.sh,
# which CI's deep.yml asks for its matrix, so no harness can be left out
# of the nightly run by a hand-kept list.
#
# Dies, naming the file and line, on an attribute it cannot pair with a
# function (other attributes and comments between them are fine), so a
# harness written in a shape this parser does not know fails loudly
# instead of vanishing.
#
#   perl scripts/lib/kani-harnesses.pl FILE...
use strict;
use warnings;

my @out;
for my $path (@ARGV) {
    my ($crate) = $path =~ m{(?:^|/)crates/([^/]+)/} or die "$path: not under crates/<crate>/\n";
    open my $fh, '<', $path or die "$path: $!";
    my @lines = <$fh>;
    close $fh;
    for my $i (0 .. $#lines) {
        next unless $lines[$i] =~ /^\s*#\[kani::proof\]/;
        my $name;
        for my $j ($i + 1 .. $#lines) {
            my $l = $lines[$j];
            next if $l =~ m{^\s*(?:#\[|//|$)};
            ($name) = $l =~ /^\s*(?:pub(?:\([^)]*\))?\s+)?(?:const\s+|unsafe\s+)*fn\s+(\w+)/;
            last;
        }
        die "$path:" . ($i + 1) . ": #[kani::proof] not followed by a function\n" unless $name;
        push @out, "$crate $name";
    }
}
my %seen;
for (@out) { die "duplicate harness: $_ (cargo kani --harness would pick both)\n" if $seen{$_}++ }
# `--harness NAME` matches by substring: one name inside another in the
# same crate would run both in the shorter one's job.
for my $x (@out) {
    for my $y (@out) {
        my ($ca, $na) = split q{ }, $x;
        my ($cb, $nb) = split q{ }, $y;
        die "harness ${ca}::${na} is part of ${cb}::${nb}'s name (cargo kani --harness $na would run both)\n"
            if $x ne $y && $ca eq $cb && index($nb, $na) >= 0;
    }
}
print "$_\n" for sort @out;
