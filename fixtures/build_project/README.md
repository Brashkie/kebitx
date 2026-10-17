A tiny add-on (one entity, both packs, clean) used by the `kebitx-build` tests. The tests copy
it to a temporary folder and write `kebitx.json` there, pointing the filter at the compiled
`kebitx-filter-example`, so this fixture is never modified.

`world/` is a world template for the `mcworld` target: a placeholder `level.dat`, a `db/`, a pack
list that activates somebody else's pack (kept) and an older version of `demo BP` (replaced), and
a stale copy of `demo_BP` (dropped from the archive).
