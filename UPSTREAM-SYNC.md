# Upstream sync ledger

`upstream` is `Drop-OSS/drop`. Its commits are cherry-picked onto `develop` as
themselves — see "Pulling in commits from upstream" in `CLAUDE.md` for how.

This file exists because `git cherry develop upstream/develop` can't answer the
one question a sync starts with. It marks a commit `+` whenever our copy's
patch differs, which covers three different situations at once: never picked,
picked and adjusted, and deliberately left out. Everything below that `git`
can't tell apart is written down here instead.

## Last synced

`24e7a889` — Load download page versions from GitHub releases (#518).

The next sync picks `24e7a889..upstream/develop`, minus anything this file
says to leave out.

## Deliberately not picked

Each of these stays out until the reason stops holding. A later upstream
commit that builds on one is judged on its own.

| Upstream | Subject | Why |
| --- | --- | --- |
| `df908490` | Implementing Playtime Tracking (#506) | We already track playtime, in more detail and first: `9f26d793` (session ledger and sync endpoints), `6f5d1619` (cross-device, and already-paired clients), `4eda7c1e` (periodic checkpointing), `c106f3d1`. Upstream's `/api/v1/client/playtime/[gameid]/` routes would collide with our `playtime/[gameid].get.ts`. |

## Picked, but not as-is

These are applied — the `+` from `git cherry` is expected, not a to-do.

| Upstream | Ours | What changed |
| --- | --- | --- |
| `d2df0466` | `8e5aecf4` | Fix devcontainer (#482). |
| `3b740630` | `a7253b0f` | Update actions (#483). |
| `6c367171` | `b6844a85` | Dropped upstream's `20260726041152` age-rating migration: ours, `20260725000000_repair_age_rating_schema`, already does it. Kept the object-storage stream-leak fixes and the Quadlet template. |
| `31508e23` | `f4518d0f` | Dropped `server/rules/no-prisma-delete.mjs`, which nothing imports — both eslint configs load our `.mts`. Kept our `invokeWithTimeout` and our uninitialised declarations (eslint `no-useless-assignment`, from `5741ce5d`) while taking upstream's IGDB null-safety fix, and merged upstream's "copy out of the cache entry before streaming" into our stale-context rebuild in `torrential`. |
| `0f426f92` | `672accdc` | Kept our dependency versions and lockfiles, which are ahead of upstream's and carry the audit work from `5741ce5d` and `ffda1154`; took `astro` and `next`, which upstream had newer. Kept `allowBuilds` (pnpm 12's name) over the `onlyBuiltDependencies` it replaced. |
