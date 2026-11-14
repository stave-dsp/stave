#!/usr/bin/env perl
# stave-dsp's layering bar (DSP.md's layer table): each layer may use only
# the layers listed before it. Read by scripts/check-view-bindings.sh.
#
#   perl scripts/lib/layering.pl SRC LAYER...
#
# SRC is a crate's `src` directory and the LAYERs are its top-level
# modules, lowest first. Prints one `path:line: LAYER -> ABOVE: text`
# line per reference from shipped code in one layer to a layer above it,
# and one `path: missing layer LAYER` line per layer directory that does
# not exist (a renamed layer must not leave the bar checking nothing).
#
# A reference is `crate::<layer>` anywhere in code, or a top-level
# segment of a grouped path: `use crate::{units::Hz, effects::Room}` names
# `units` and `effects`, across lines.
#
# Shipped code is everything but test and proof code. Test-only items are
# found by their attribute, `#[cfg(test)]` or `#[cfg(kani)]` (also inside
# `any`/`all`): the attribute covers the ONE item that follows it — a
# `use`, a `fn`, an inline `mod { … }` up to its closing brace — and
# `mod name;` excludes that module's file and directory. `#![cfg(test)]`
# excludes its whole file. Test modules may compose across layers (a
# resampler test can drive a triode as its fixture).
#
# Comments, strings and character literals are blanked first (keeping
# line breaks), so braces and paths inside them neither count nor hide.
use strict;
use warnings;
use File::Find;

my ($src, @layers) = @ARGV;
die "usage: $0 SRC LAYER...\n" unless defined $src && @layers;
$src =~ s{/+$}{};
my %rank;
@rank{@layers} = 0 .. $#layers;

my @files;
for my $layer (@layers) {
    my $dir = "$src/$layer";
    unless (-d $dir) { print "$dir: missing layer $layer\n"; next }
    find({ no_chdir => 1, wanted => sub { push @files, $_ if /\.rs$/ && -f $_ } }, $dir);
}
@files = sort @files;

# A test-only attribute: cfg(test), cfg(kani), or any/all over them
# (`all(test, feature = "x")` is test-only too; `any(test, feature = "x")`
# is not, and stays checked).
my $TESTCFG = qr/\#\[\s*cfg\s*\(\s*(?:test|kani|any\s*\(\s*(?:test|kani)(?:\s*,\s*(?:test|kani))*\s*,?\s*\)|all\s*\(\s*(?:test|kani)\b[^\]]*\))\s*\)\s*\]/;

my (%clean, %test_ranges, %excluded);

# Blank comments, strings and char literals with spaces, keeping newlines.
sub blank { my $s = shift; $s =~ s/[^\n]/ /g; $s }
sub scrub {
    my $s = shift;
    $s =~ s{
        ( //[^\n]* )                                   # line comment
      | ( /\* (?: [^*]++ | \*(?!/) )*+ \*/ )           # block comment (unnested)
      | ( b?r(\#*)" .*? "\4 )                          # raw string
      | ( b?" (?: [^"\\]++ | \\. )*+ " )               # string
      | ( b?' (?: [^'\\\n] | \\[^\n]{1,10}? ) ' )      # char literal (not a lifetime)
    }{ blank($&) }gsex;
    $s;
}

# The end offset (exclusive) of the item starting at or after $pos: skip
# outer attributes, then stop at a `;` before any brace, or at the brace
# that closes the first one opened.
sub item_end {
    my ($s, $pos) = @_;
    my $n = length $s;
    while (1) {
        pos($s) = $pos;
        if ($s =~ /\G\s*\#\[/gc) {
            my $depth = 1;
            my $i = pos($s);
            while ($i < $n && $depth) {
                my $c = substr($s, $i++, 1);
                $depth++ if $c eq '[';
                $depth-- if $c eq ']';
            }
            $pos = $i;
            next;
        }
        last;
    }
    my $depth = 0;
    for (my $i = $pos; $i < $n; $i++) {
        my $c = substr($s, $i, 1);
        if ($c eq '{') { $depth++ }
        elsif ($c eq '}') { $depth--; return $i + 1 if $depth == 0 }
        elsif ($c eq ';' && $depth == 0) { return $i + 1 }
    }
    return $n;
}

# The module path a `mod name;` declared in $file resolves to.
sub child_module {
    my ($file, $name) = @_;
    (my $dir = $file) =~ s{/[^/]+$}{};
    return "$dir/$name" if $file =~ m{/(?:mod|lib|main)\.rs$};
    (my $stem = $file) =~ s{\.rs$}{};
    return "$stem/$name";
}

for my $file (@files) {
    open my $fh, '<', $file or die "$file: $!";
    local $/;
    my $s = scrub(<$fh> // q{});
    close $fh;
    $clean{$file} = $s;
    my @ranges;
    if ($s =~ /\#!\[\s*cfg\s*\(\s*(?:test|kani)\s*\)\s*\]/) {
        push @ranges, [0, length $s];
    }
    while ($s =~ /$TESTCFG/g) {
        my $start = $-[0];
        my $end = item_end($s, $+[0]);
        push @ranges, [$start, $end];
        my $item = substr($s, $+[0], $end - $+[0]);
        if ($item =~ /^\s*(?:\#\[[^\]]*\]\s*)*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*;\s*$/s) {
            $excluded{ child_module($file, $1) } = 1;
        }
        pos($s) = $end;
    }
    $test_ranges{$file} = \@ranges;
}

sub is_excluded {
    my $file = shift;
    for my $m (keys %excluded) {
        return 1 if $file eq "$m.rs" || index($file, "$m/") == 0;
    }
    return 0;
}

sub in_test {
    my ($file, $off) = @_;
    for my $r (@{ $test_ranges{$file} }) {
        return 1 if $off >= $r->[0] && $off < $r->[1];
    }
    return 0;
}

sub line_of { my ($s, $off) = @_; 1 + (substr($s, 0, $off) =~ tr/\n//) }

my @out;
sub report {
    my ($file, $own, $above, $off) = @_;
    my $line = line_of($clean{$file}, $off);
    open my $fh, '<', $file or die "$file: $!";
    my @lines = <$fh>;
    close $fh;
    my $text = $lines[$line - 1] // '';
    chomp $text;
    push @out, [$file, $line, "$file:$line: $own -> $above: $text\n"];
}

for my $file (@files) {
    next if is_excluded($file);
    my ($own) = $file =~ m{^\Q$src\E/([^/]+)/};
    next unless defined $own && exists $rank{$own};
    my $s = $clean{$file};
    my %seen;
    my $hit = sub {
        my ($name, $off) = @_;
        return unless exists $rank{$name} && $rank{$name} > $rank{$own};
        return if in_test($file, $off);
        return if $seen{"$off"}++;
        report($file, $own, $name, $off);
    };
    # crate::layer
    while ($s =~ /\bcrate\s*::\s*(\w+)\b/g) {
        $hit->($1, $-[1]);
    }
    # crate::{ a::…, b, c::{…} } — the top-level segments
    while ($s =~ /\bcrate\s*::\s*\{/g) {
        my $i = $+[0];
        my $depth = 1;
        my $want = 1;    # at the start of a top-level segment
        my $n = length $s;
        while ($i < $n && $depth) {
            my $c = substr($s, $i, 1);
            if ($c eq '{') { $depth++; $want = 0; $i++; next }
            if ($c eq '}') { $depth--; $i++; next }
            if ($c eq ',' && $depth == 1) { $want = 1; $i++; next }
            if ($want && $depth == 1 && substr($s, $i) =~ /^(\w+)/) {
                $hit->($1, $i);
                $want = 0;
                $i += length $1;
                next;
            }
            $want = 0 if $c =~ /\S/;
            $i++;
        }
    }
}

print map { $_->[2] } sort { $a->[0] cmp $b->[0] || $a->[1] <=> $b->[1] } @out;
