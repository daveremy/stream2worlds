# 0017: Visualization and agent use are first class, built with the core

Date: 2026-09-28 · Status: accepted (Dave-directed) · Issues #114, #116, #117

## Decision

Visualization and agent use are first-class parts of S2W, not layers added after the core.
Dave, 2026-09-28: *"i want to co develop visualization with the core functionality so we keep
it first class. visualization is first class in s2w. that being said so is agent usage."*

So **every gate ships three surfaces together**: the core capability, the view that shows it to
a person, and the MCP surface that shows it to an agent. A gate that ships a capability with no
view or no agent surface is not done. Where a surface truly cannot exist yet, the gate records
why and names the issue that adds it.

## Why

The demo after Gate 2's web view (2026-09-28) was correct but not compelling: unlabeled dots
and raw JSON, while the world behind it already held typed entities with titles and names.
The core had outrun what anyone could see. A low-level tool tends to treat its dashboard as an
afterthought. For S2W the view is how people decide whether to trust the world, and agents are
expected to do most of the reading.

## The direction: a domain-specific dashboard from a domain-free core

This is how a generic engine can still show each domain in its own terms. Research note 0008
(sagan) tests it against prior art.

1. **The dashboard is data, not code.** System 2 writes a constrained, validated **view spec**:
   the primary entities; the live-model form (graph, map, timeline, state flow or table); panels
   and the query behind each; labels; icons from a fixed set; and color roles. A generic
   renderer draws it. This is the rule-language pattern again: a small, checkable language that
   LLMs write well.
2. **The view spec is an event in the log.** It is versioned and can be revoked, and it changes
   as understanding grows. Scrubbing back shows the dashboard as the world was understood then.
3. **Presentation only.** System 2 chooses what to show and how, never the numbers. Every panel
   is a query over the world, so a view spec cannot invent a fact.
4. **Shape before domain.** System 1 detects generic shapes (latitude/longitude pairs, repeated
   state transitions, hubs, rates), and System 2 names the domain and chooses the form. The
   obfuscation test applies: renamed fields should yield the same dashboard.
5. **A lifetime baseline, and surprise against it.** System 1 learns what is normal, incrementally
   and for the life of the stream: rates per event type, fields, cohorts, transitions. Surprise
   is how improbable an event is under that baseline: a new field, a never-seen transition, a
   rate break, an entity acting unlike its cohort, a new hub.
6. **From surprise to questions.** System 2 reads the surprise feed and proposes questions worth
   tracking, such as "pages reverted once get reverted again within the hour; track this?" A
   question the user accepts becomes a registered forecast and is graded. The graded record
   keeps discovered "insight" from turning into hallucination. This is how S2W can show people
   what they did not know to ask.
7. **The agent sees what the person sees.** The same view spec, the attention feed and the
   evidence are served over MCP, so an agent can explain the dashboard it is looking at.

## Sequencing (karpathy)

| When | View | Agent |
|---|---|---|
| Gate 2 (now) | #114: a domain-free view (labels, types, activity, hubs) from world data alone | `--json` everywhere (#110); MCP over the real world (#115) |
| Before Gate 3 | View spec v0: the schema and a generic renderer; the spec is derived by System 1 shape rules and stored as an event | The view spec and attention feed over MCP |
| Gate 3 | System 2 writes and revises the view spec, including on the obfuscated copy | System 2 through the client agent (MCP sampling) |
| Gate 3 → 4 | Baseline and surprise panel (#117) | `attention()` over MCP |
| Gate 4 | Accepted surprise questions become graded forecasts | `forecast.ask` for registered questions |

## Consequences

- Sprint planning pairs view and agent items with each core item; the sprint's demo is how the
  pairing shows.
- `CLAUDE.md` carries the three-surface rule, so agents building gates apply it.
- The view renderer must stay generic. Anything domain-specific belongs in a view spec, never in
  renderer code.
