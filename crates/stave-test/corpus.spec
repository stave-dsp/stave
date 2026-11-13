# The reference corpus (DSP.md R22): what program material stave's checks
# need, by role, and the rules that pick it from the sample library
# STAVE_SAMPLE_LIBRARY names. No audio is committed, and no library path:
# only roles and library-relative selection rules. `scripts/corpus.sh lock`
# records each pick's SHA-256 locally (never committed) so a run names
# exactly what it played; `scripts/corpus.sh check` reports missing roles
# and checksum drift. Readers: `stave_test::corpus` and scripts/corpus.py.
#
# A role is a `role NAME` line and indented `key value` lines:
#
#   about       what it is and why the checks need it
#   instrument  the source instrument (free text)
#   technique   how it was played or recorded (free text)
#   used-by     the DSP.md items that read it
#   required    yes: `corpus.sh check` fails without it; no: reported only
#   select      where to look, in order (several lines; picks are de-duplicated):
#                 file PATH       one library-relative file
#                 set NAME        a set of _corpus.json (else `Test Corpus/NAME/`)
#                 dir PATH        every WAV under a library directory
#                 manifest GLOB   _manifest.csv rows whose category matches GLOB
#               then `; key=value` filters: name=GLOB (the file name),
#               kind=TEXT (manifest only), min-seconds, max-seconds,
#               min-rate, channels. Only WAV is read: AIFF (Ableton's
#               protected AIFF-C among it) and `._*` sidecars never are.
#   combine     sum DEPTH: files sharing their first DEPTH path components
#               are one item, summed at unity (stems into a mix bus)
#   count       how many items the role takes (the first, in order)
#   excerpt     START SECONDS: the part of each item a check plays
#   fill        how to fill the role when the library has nothing for it
#
# Globs: `*` any run of characters, `?` one. Paths use `/`.

role guitar_di
  about       a clean electric guitar straight from the pickup, unprocessed: what amps, pedals and cabs are built to take, edge of breakup included
  instrument  electric guitar (single coil and humbucker)
  technique   DI: single notes, chords, palm-muted, soft to hard picking
  used-by     C13 M4 R21
  required    no
  select      dir Source Material/DI/Guitar; min-rate=44100; min-seconds=4
  count       8
  excerpt     0 20
  fill        record guitar DI takes (DSP.md R22: eight takes, about four minutes) into Source Material/DI/Guitar

role bass_di
  about       a clean bass guitar straight from the pickup, unprocessed
  instrument  electric bass
  technique   DI: fingered, picked, slapped, low sustain
  used-by     C13 M4 R21
  required    no
  select      dir Source Material/DI/Bass; min-rate=44100; min-seconds=4
  count       4
  excerpt     0 20
  fill        record bass DI takes (DSP.md R22: four takes, about two minutes) into Source Material/DI/Bass

role bass_clean
  about       the clean side of a split bass in a real multitrack: close to a DI but not verified as one (mixed, edited, 44.1 kHz)
  instrument  electric bass
  technique   clean split, in context
  used-by     C13 R21
  required    no
  select      manifest Stems/*/BASS; name=*CLEAN*.wav; min-seconds=10
  count       2
  excerpt     30 20

role guitar_amped
  about       electric guitar already through an amp and cab: chops, riffs and wah loops
  instrument  electric guitar
  technique   amped, recorded loops
  used-by     C13 R21
  required    yes
  select      file Test Corpus/guitar/Beefy Chop F#.wav
  select      set guitar
  count       4

role guitar_stem
  about       electric guitar tracks from real multitracks, amped, in context
  instrument  electric guitar
  technique   amped multitrack stems
  used-by     C13 R21
  required    no
  select      manifest Stems/*/GUITARS; name=*GTR*.wav; min-seconds=10
  count       4
  excerpt     30 20

role drums_loop
  about       full-kit and percussion loops: transients with a groove
  instrument  drums
  technique   loops
  used-by     C13 M4 R21
  required    yes
  select      set drum loops
  count       8

role drums_stem
  about       drum overheads from real multitracks
  instrument  drums
  technique   overhead pair
  used-by     C13 R21
  required    no
  select      manifest Stems/*/DRUMS; name=OVERHEADS.wav; min-seconds=10
  count       2
  excerpt     30 20

role transients
  about       single hits: kicks, snares, hats, shakers — attack and decay
  instrument  drums and percussion
  technique   one-shots
  used-by     C13 R21
  required    yes
  select      file Test Corpus/snares/Snare Crack It.wav
  select      set kicks
  select      set snares
  select      set hats
  select      set percussion
  count       14

role mix_bus
  about       every stem of one multitrack summed at unity: a mix bus before bus processing
  instrument  band
  technique   stems summed
  used-by     C13 M4 R21
  required    yes
  select      manifest Stems/*; min-seconds=10
  combine     sum 2
  count       2
  excerpt     30 30

role vocal
  about       sung and spoken voice, one-shots and phrases
  instrument  voice
  technique   dry and produced vocal takes
  used-by     C13 R21
  required    yes
  select      set vocal phrases
  select      set vocals
  count       7

role piano
  about       grand piano notes across the range, forte
  instrument  piano
  technique   single notes
  used-by     C13 R21
  required    yes
  select      set piano notes
  count       6

role synth_pad
  about       slow sustained synth swells and bells: smooth, wide-band, steady
  instrument  synthesiser
  technique   pads and swells
  used-by     C13 R21
  required    yes
  select      file Test Corpus/tonal loops/Synth Swell F.wav
  select      file Test Corpus/tonal loops/Atmos Dark Bells.wav
  count       2

role synth_notes
  about       synth multisample notes, bass to lead
  instrument  synthesiser
  technique   single notes
  used-by     C13 R21
  required    yes
  select      set synth notes
  count       4

role tonal_loop
  about       melodic loops: guitar, keys, synth, voice
  instrument  mixed
  technique   loops
  used-by     C13 R21
  required    yes
  select      set tonal loops
  count       8

role texture
  about       noise-like textures and ambiences: dense spectra with no pitch
  instrument  field and foley
  technique   textures
  used-by     C13 R21
  required    yes
  select      set textures
  count       4

role fx
  about       risers, impacts and horns: extremes of level and bandwidth
  instrument  sound design
  technique   one-shots
  used-by     C13 R21
  required    yes
  select      set fx
  count       3
