# Source facts and portable keyboard interpretation

Status: Implemented; local validation and CI acceptance in progress. [Specification #1565](https://github.com/vanyastaff/flui/issues/1565).

## Existing seams

Actual DOM dispatch reaches the retained public on_input callback. Public runtime
input dispatch reaches mounted Focus/EditableText, Shortcuts/CallbackShortcuts and
primary-focus Actions. Pointer-focused Slider proposals use the existing public
focus seam. These are separate acceptance seams, not a rendered browser
end-to-end claim.

## Source and interpretation

Platform adapters own observations; the contract crate owns the representation;
owner/runtime transport complete events; widget consumers own keyboard policy.
No public DTO changes, extra trait/layer, global keyboard manager, upstream fork
or Unicode heuristic is required.

Web gathers canonical getModifierState values for keyboard and MouseEvent-based
inputs. Positive source observations and convenience booleans are preserved;
supported legacy Meta aliases normalize to the owned representation. Existing
source grammar, event metadata and retirement fences remain.

The widget layer projects four ordinary command dimensions from raw Modifiers.
Explicit AltGraph removes Control/Alt from this private view, while Meta and
Shift remain. Locks/Fn/Symbol remain raw observations but do not qualify as extra
commands. SingleActivator, field command/word movement and Slider use this one
interpretation. It changes no source event.

| Input | Framework behavior |
|---|---|
| Explicit AltGraph with optional Control/Alt, no Meta | Character insertion in field; produced-character shortcuts outside field; no alias command |
| Explicit AltGraph plus Meta | Meta command priority, no field insertion |
| Ctrl+Alt without explicit AltGraph | Existing conservative field text fallback; ordinary activator can match on an unhandled focused leaf |
| Source composition Active | No ordinary editing/shortcut/control action; key remains unclaimed for native defaults |
| Lock-bearing ordinary command/navigation | Same command or Slider proposal as without locks |
| KeyUp | No ordinary activator or field insertion |

Shift policy, ASCII letter comparison, complete character strings, repeat and
action resolution remain as specified by ADR-0023/ADR-0156 except the explicitly
changed command interpretation. Named word movement also excludes AltGraph
aliases. Raw Focus handlers can still inspect full observations and intentionally
implement another policy.

## Composition and recovery

Source composition and controller composition are independent authorities. An
Active source flag protects the key even before a controller has a composing
range; Inactive cannot cancel a controller's range. Activator and field/control
decisions must retain native/default handling for unclaimed keys.

No stateful modifier tracker is added. Host getter calls run without guards;
reentrant inner events can complete before an outer observation. Old
registrations remain fenced after close/replacement. Existing FIFO, callback
retirement and lifecycle contracts govern accepted delivery. Default wasm
panic-abort and arbitrary JavaScript getter exceptions are not claimed as Rust
unwind recovery.

## Migration and durable decisions

A scoped successor ADR supersedes affected matching policy in ADR-0023/ADR-0156;
other action/dispatch and owned-vocabulary contracts remain. Docs distinguish
active toggle observations from command dimensions and retain missing-AltGraph
compatibility and native limits. Platform source observation and widget
interpretation migrate together before this feature is considered complete.
