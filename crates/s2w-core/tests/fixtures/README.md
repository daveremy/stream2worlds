# Golden replay fixtures (human-owned)

- `golden-fold-v1.json`: a hand-written log of `WorldEvent`s. It uses every variant and makes
  `enwiki` a hub.
- `golden-fold-v1.snapshot.json`: the world that log folds to, as pretty JSON plus a newline.

`cargo xtask check` folds the log from `World::with_hub_cap(3)`, not the production default of
10,000, so the hub fits in a log a person can read. It tests fold behaviour at the cap, not the
default. The check never rewrites the snapshot. When a fold change is intentional, a human
reviews the new world and edits the snapshot in the same PR (decision 0004).
