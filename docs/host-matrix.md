# Host matrix — manual verification checklist

One pass per release. Machine checks
cover the ABI contract; this covers what only a real DAW exercises:
automation record/playback/undo feel, editor embedding, session
interchange. Check a box only after performing the row in that host on
current main.

Per host, per plugin (every plugin in `plugins/`; AU is not supported yet,
so Logic is out of scope until it is):

| Check | Live 12 | Reaper 7 | Bitwig 5 |
|---|---|---|---|
| Loads + passes host's plugin scan | ☐ | ☐ | ☐ |
| Editor opens, knobs move DSP | ☐ | ☐ | ☐ |
| Knob drag records automation (gesture path) | ☐ | ☐ | ☐ |
| Recorded automation plays back sample-stably | ☐ | ☐ | ☐ |
| Undo after knob drag restores prior value | ☐ | ☐ | ☐ |
| Save project → reopen → identical state | ☐ | ☐ | ☐ |
| Preset/program list visible where supported | ☐ | ☐ | ☐ |
| Bypass toggle is click-free | ☐ | ☐ | ☐ |
| Two instances on two tracks stay independent | ☐ | ☐ | ☐ |
| Param values display with units in host UI | ☐ | ☐ | ☐ |

Notes / host-specific findings (append, dated):

- (none yet)
