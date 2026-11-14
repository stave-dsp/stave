#!/usr/bin/env perl
use strict;
use warnings;
# An operator inventory chooses the environment; never assume rustup layout.
my ($file, $host) = @ARGV;
defined($file) && defined($host) or die "usage: offload-env.pl MANIFEST HOST\n";
open my $fh, '<', $file or die "offload: cannot read environment manifest $file: $!\n";
my %env;
while (<$fh>) {
    chomp; s/\r$//;
    next if /^\s*(?:#|$)/;
    my @fields = split /\t/, $_, -1;
    @fields == 2 or die "offload: expected two tab-separated fields at $file line $.\n";
    my ($name, $path) = @fields;
    $name =~ /\A[A-Za-z0-9_.\@-]+\z/ or die "offload: invalid host at line $.\n";
    $path =~ /\A[A-Za-z0-9_.-]+(?:\/[A-Za-z0-9_.-]+)*\z/ && !grep { $_ eq '..' || $_ eq '.' } split('/', $path)
        or die "offload: invalid HOME-relative env path at line $.\n";
    !exists($env{$name}) or die "offload: duplicate host $name\n";
    $env{$name} = $path;
}
close $fh or die "offload: manifest close failed: $!\n";
exists($env{$host}) or die "offload: no declared environment for $host in $file\n";
print '. "$HOME/' . $env{$host} . '"';
