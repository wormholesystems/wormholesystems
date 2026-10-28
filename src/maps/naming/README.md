# Naming strategies

A map's naming strategy decides two things: the alias a newly reached system gets, and the
in-game bookmark name for a hole. The server does all the naming, and the web app and the
desktop app only ask it, so a new convention is written once, here, in Rust.

Each strategy has its own file:

- `chain.rs`: the default. An alias extends the one it was reached from (`1`, `11`, `112`),
  so the name itself says which way home is.
- `siblings.rs`: "Per system" in the settings. Each system numbers its own holes `1, 2, 3`,
  and the map's distance from home decides which bookmark is the way back.

If your group names things some other way, you can add a strategy for it and open a pull
request.

## The trait

A strategy implements `NamingStrategy` from `mod.rs`:

```rust
pub trait NamingStrategy: Sync {
    fn alias(&self, map: &NamingMap, origin: &Node, target: &Target) -> Option<String>;
    fn bookmark(&self, map: &NamingMap, destination: &Destination, hole: &Hole, near: Option<&Node>) -> String;
    fn example_aliases(&self, settings: &MapNaming) -> Vec<String>;
}
```

- `alias` names a system reached from `origin`. `target` holds whatever is known about it
  (its class or its security, or neither for an unscanned hole). Return `None` to leave it
  unnamed, the way the chain leaves plain k-space travel alone.
- `bookmark` gives the bookmark text for `destination`, written while standing in `near` at
  the other end of the hole. `near` is `None` when there's no such system on the map.
- `example_aliases` lists the first few aliases on a fresh map, for the preview on the
  settings page.

`NamingMap` is the whole map: its settings (`map.settings`), the nodes with their aliases and
systems, the connections, `neighbours(id)` and `distances_from_home()`. Ghosts (holes nobody
has been through yet) are nodes without a system, and their aliases count as taken too.

A strategy is pure. It gets no database, no clock and no request, and it doesn't save
anything: the server claims the alias it returns under a lock, so two pilots jumping at once
can't get the same name. That keeps a strategy easy to test, because a test only has to build
a `NamingMap`.

## Pieces you can reuse

- `chain::leg` tells a hop that's part of the chain from plain k-space travel, and gives you
  the reserved letter of a k-space exit (`H`, `L`, `N`, `P`).
- `chain::render_with_formats` renders a bookmark through the map's three bookmark formats,
  so users' custom formats keep working. Use it unless your convention really needs its own
  format.
- `chain::lowest_free` and `siblings::next_sibling` find the lowest free number or letter.
- `bookmark.rs` has the class helpers (`class_short`, `class_from_security`) and
  `render_bookmark` for a template of your own.

## Adding one

1. Add `src/maps/naming/yours.rs` with a unit struct that implements `NamingStrategy`, and
   `pub mod yours;` in `mod.rs`.
2. Add a variant to `NamingStrategyKind` in `src/maps/mod.rs`, and a migration that adds the
   value to the Postgres enum:

   ```sql
   alter type naming_strategy add value 'yours';
   ```

3. Return it from `strategy()` in `mod.rs`. The match is exhaustive, so the compiler points
   you there.
4. Regenerate the TypeScript types with `cargo test export_bindings`, then add a toggle for
   it to `frontend/src/routes/maps/[id]/settings/naming/NamingCard.svelte`, with a one-line
   description.
5. Add a page under `frontend/src/docs/03-bookmarking/` that explains the convention to the
   people who'll use it. `09-per-system-numbering.md` is a good template.

The desktop app asks the server too, so it needs no changes.

## Tests

Put unit tests at the bottom of your file, the way `siblings.rs` does: build a small map by
hand, then check the aliases it suggests and the bookmarks it writes in both directions. The
`farm` map there is a real chain from a user's screenshot. A map from your own group's chain
is the best test you can write, because it shows the convention doing what you actually need.

Run them with `cargo test --lib naming::`.

Before writing a lot of code, consider opening an issue that describes the convention with an
example chain. It's an easy way to check whether one of the existing strategies, with a
setting or two, would already cover it.
