---
title: Contributing
category: Contributing & self-hosting
---

# Contributing

WormholeSystems is built in the open, and contributions of every size are welcome. It is
early enough that almost anything is still up for discussion.

## The repository

**[wormholesystems/wormholesystems](https://github.com/wormholesystems/wormholesystems)**
— the map, the API and the operator CLI, in one place. Rust on the back, SvelteKit on the
front.

Start with the repo's README, then:

- **Found a bug or have an idea?** Open an issue. It is the best place to work out whether
  a change is right before anyone builds it.
- **Want to fix or build something?** Fork it, make the change, open a pull request.

Because this is still beta, expect the ground to move under a branch that sits open for
long. Small changes land more easily than large ones.

## Improving these docs

The documentation you are reading is the easiest place to start. Every page is a plain
Markdown file under `frontend/src/docs/`, one folder per category, and adding a page needs
nothing but a file with a title in it — the sidebar, the ordering and the previous/next
links all follow on their own.

Writing for somebody flying, rather than somebody reading the source, is the only rule
worth stating.

## Your own naming convention

If your group names its holes in a way neither
[the standard convention](/documentation/bookmarking/why-this-system) nor
[per-system numbering](/documentation/bookmarking/per-system-numbering) covers, it can be
added as a strategy of its own. A strategy is one Rust file that decides the alias a new
system gets and the bookmark name for a hole, and every map can then pick it in
**Map settings → Naming**. The
[naming README](https://github.com/wormholesystems/wormholesystems/tree/main/src/maps/naming)
explains how to write one and what it gets to look at.

## Why bother

- **It helps everyone.** A fix or a page you add is one every group using it gets.
- **It is yours too.** The same openness that lets you
  [self-host](/documentation/contributing-and-self-hosting/self-hosting) lets you shape
  where it goes.
