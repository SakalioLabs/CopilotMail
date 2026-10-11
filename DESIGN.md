# CopilotMail Design Contract

> Status: active implementation contract. This file is the single source of truth for
> visual direction, interaction principles, and the rules that keep the Human and
> Agent experiences coherent.

## Direction

**Measured Agent Workbench** is the one primary product style: a compact,
evidence-led workspace with deliberate hierarchy, a controlled jade action
signal, and no visual noise. The Human Mail surface keeps a keyboard-first,
operational posture, but “Superhuman Operational” describes that interaction
posture only; it is not a second visual language or a third-party visual
reference to copy.

The Agent identity language uses original warm-paper, graphite-ink, and bounded
linear-routing motifs. Jade indicates an enabled path or focus, never identity
or success by itself. The language is inspired by the restraint of modern Agent
workspaces, but it must not copy a third-party logo, mascot, or brand silhouette.

The default workspace is light. Dark mode is a later, token-derived theme; it is
not an excuse to add glow, gradients, or a second visual language.

## Product principles

1. **Operational before ornamental.** Every visual element must explain identity,
   scope, status, risk, or the next useful action.
2. **Human and Agent are both first-class.** Human, Agent, Shared, Group, and
   System identities use a shape, an icon, and a text label. Colour never carries
   identity by itself.
3. **Truthful UI only.** Do not render an action, state, timeline, or attribution
   until an API contract supplies it. In particular, do not infer that a message
   came from an Agent just from an address.
4. **One primary action per view.** Secondary work is placed in an overflow menu,
   command palette, or an on-demand inspector.
5. **Keyboard-first, pointer-complete.** A human can move, open, search, compose,
   reply, and return without a mouse. Touch targets remain at least 44px on mobile.
6. **No placeholder design.** No lorem ipsum, fake people, fake avatars, permanent
   skeletons, dead controls, empty decorative cards, blobs, or arbitrary colour
   panels are allowed in production UI.

## Visual foundation

All production colour values are defined by the shared design-system token layer.
Components consume semantic tokens; raw colours are not introduced in feature CSS.

| Role | Token | Initial value | Meaning |
| --- | --- | --- | --- |
| Paper | `--cm-canvas` | `#F8F8F6` | Warm page background |
| Surface | `--cm-surface` | `#FFFFFF` | Reading, compose, and data surfaces |
| Ink | `--cm-ink` | `#17202A` | Primary text and primary action |
| Muted ink | `--cm-muted` | `#667085` | Supporting information |
| Divider | `--cm-line` | `#E1E6E3` | Structural boundaries only |
| Control boundary | `--cm-control-border` | `#7A8795` | Perceivable form and secondary-action outline, at least 3:1 against white |
| Agent/action signal | `--cm-agent` | `#0F766E` | Focus, enabled action, factual Agent relation, selected route |
| Positive | `--cm-positive` | `#18794E` | Completion/delivery success |
| Warning | `--cm-warning` | `#996300` | Pending/manual review |
| Critical | `--cm-critical` | `#B42318` | Failure/destructive action |
| Info | `--cm-info` | `#245FC7` | Neutral system information |

Status colours are compact signals, not large background blocks. The normal
workspace should read as paper, white, black, and gray.

Use the local `--cm-font-sans` stack consistently across Admin and Mail; it
includes Latin and Chinese system fallbacks and never depends on a font CDN.
Use `--cm-font-mono` only for shortcuts, IDs, hashes, traces, and audit
evidence. Use a 4px micro-unit with an 8px layout rhythm, semantic type/leading
tokens, 8px controls, 12px panels, restrained elevation, and the shared stacking
scale. Feature CSS must not create a private font, spacing, motion, touch-target,
or z-index system.

## Workspace architecture

Desktop uses a compact three-column shell with an optional fourth inspector:

1. **Scope rail:** product mark, grouped mailbox entities, current sender identity,
   navigation, and session actions.
2. **Message queue:** search, true filters, list navigation, real metadata, and
   batch affordances only after the corresponding APIs exist.
3. **Reading/compose area:** message details, an action bar limited to executable
   actions, compose identity, draft state, and delivery state.
4. **Context inspector:** collapsed by default; shows real Agent ownership, scope,
   policy outcomes, delivery trace, and evidence when the API provides them.

On tablet/mobile, the queue and detail become an explicit list-detail stack. Do
not reserve a blank desktop detail column for drafts or sent items whose contracts
do not expose detail data.

At widths of 1007px or below, mail uses an explicit list-detail stack and its
controls use a minimum 44×44px target independently of the pointer type
reported by the browser. This narrow-screen override does not enlarge compact
desktop control density.

## Identity language

| Kind | Visual grammar | Required text |
| --- | --- | --- |
| Human | circular profile/outline icon | `Human` or person name |
| Agent | rounded mail core plus execution rails | `Agent` plus owner/scope when available |
| Shared | paired inbox/tray icon | `Shared` |
| Group | routed-node mailbox icon | `Group` |
| System | bounded system/core icon | `System` |
| Delegated action | actor + directional route | `on behalf of …` |

`IdentityBadge` supplies the original SVG mark and its identity shape. Its `label`
is mandatory, visible, and localized for the current interface; it never invents
an English fallback, avatar, owner, scope, delegation, or status. A persisted
`Service` actor uses the distinct `service` identity kind; its bounded core glyph
is only visual grammar and its visible wording remains `Service` (or the localized
equivalent), never `System`.

`StatusIndicator` is for API-backed state only. It always combines a semantic
glyph, a compact shape/tone, and visible localized text; live announcements are
enabled only for an actual asynchronous state change. Agent state never hides
behind animation or a colour-only signal and, where available, includes a reason
and next action.

## Icons and imagery

Functional icons are hand-authored SVG in the shared icon registry:

- 24×24 view box, 1.75px stroke, round caps and joins.
- No Unicode/emoji icon strings in translation catalogs.
- Core entity and governance icons are original. A generated image may be used as
  a style reference, never as a 16px UI icon.

Raster imagery is restricted to the sign-in/onboarding moment and meaningful
empty/review states. Each image must have a manifest record, alt text, a concrete
screen/state/action mapping, and an optimized product derivative. See
`docs/IMAGE2_ASSET_MANIFEST.zh-CN.md`.

## Interaction contract

The first keyboard set must only expose existing operations:

| Key | Action |
| --- | --- |
| `c` | Compose |
| `/` | Focus search |
| `j` / `k` | Move message selection |
| `Enter` | Open selection |
| `Escape` | Return/close the active transient surface |
| `r` | Reply when a replyable message is open |
| `s` | Toggle star when a message is selected |
| `g i` / `g d` / `g s` | Inbox / drafts / sent |

Do not claim that accepted submission means delivered. Preserve the current
distinction among local draft, accepted submission, reconciliation, delivery,
failure, and manual review.

## Implementation constraints

- Preserve the existing request-generation isolation, abort behavior, draft CAS,
  submission locking, secure plain-text rendering, and focus restoration.
- Extract controllers/hooks before adding visual state to the monolithic app shell.
- The shared design system owns tokens, primitives, SVG icons, entity badges,
  status indicators, dialogs, and command surfaces.
- The mail app owns mail-specific orchestration and API contracts.
- The admin app adopts the same tokens before it receives any new visual features.
- Screens must be verified with real long subjects, multiple recipients,
  attachments, readonly mailboxes, errors, empty data, unknown submissions, and
  both Chinese and English.

## Acceptance gates

- WCAG 2.2 AA contrast; visible focus; reduced motion; keyboard completion of
  core workflows; no identity conveyed by colour alone.
- The shared gate calculates the critical text/control/focus token pairs. Focus
  uses an opaque surface-isolation ring plus Agent-colour outer ring, and
  reduced-motion mode stops indefinite spinners instead of merely accelerating them.
- No raw colour literals outside the token definition layer.
- No non-semantic decorative asset and no production placeholder string.
- Screenshot regression at 1440, 1280, 1024, 768, and 390px.
- Existing API parsing, security, and asynchronous-state tests remain green.
- Images are compressed before production use; a 2× empty-state illustration must
  be no larger than 120KB and the login hero no larger than 300KB.

## Related specifications

- `docs/DESIGN_SYSTEM.zh-CN.md`
- `docs/DESIGN_DEVELOPMENT_SPEC_V2.zh-CN.md`
- `docs/DESIGN_D0_ADMIN_FIELD_MAPPING.zh-CN.md`
- `docs/DESIGN_D4_MAIL_FIELD_MAPPING.zh-CN.md`
- `docs/DESIGN_D4B_QUEUE_READING_MAPPING.zh-CN.md`
- `docs/DESIGN_D4C_DRAFTS_SENT_MAPPING.zh-CN.md`
- `docs/DESIGN_D4D_COMPOSE_MAPPING.zh-CN.md`
- `docs/DESIGN_DEVELOPMENT_BLUEPRINT.zh-CN.md`
- `docs/HUMAN_AGENT_UX_SPEC.zh-CN.md`
- `docs/IMAGE2_ASSET_MANIFEST.zh-CN.md`
- `docs/IMPLEMENTATION_PLAN.zh-CN.md`
