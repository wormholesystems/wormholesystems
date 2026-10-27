---
title: Per-system numbering
---

# Per-system numbering

The [standard convention](/documentation/bookmarking/why-this-system) builds each alias on the one before it, so a hole three jumps out reads `213`. Some groups would rather keep every name short: each system numbers its own holes `1, 2, 3`, and one further out starts at `1` again. A manager can switch the map to this in **Map settings → Naming → Strategy → Per system**.

Like switching schemes, this only changes what the mapper **suggests** from then on. Nothing already on the map is renamed.

## The convention

- **Each system numbers its own holes**: home's are `1, 2, 3`; the holes inside its `1` are `1, 2` again, and so are the ones inside its `2`.
- **The way you came in doesn't count**: the hole back toward home is not one of a system's own, so it never takes a number from them.
- **Freed numbers are reused**: with `1` and `3` left after the `2` was rolled, the next hole is `2`.
- **The alphabetical scheme works the same way** with letters, `A, B, C`, and k-space exits take their reserved letter and a number, `H1, H2`.

## Set a home system

A chain alias says which way home is on its own: `21` sits inside `2`. A per-system `1` doesn't, so the mapper works the way home out from the map instead, and for that it needs a **home system**. With one set:

- the bookmark back toward home gets the return format (`*1 ABC C5` by default), the same as in the standard convention;
- a system's parent is never counted among its children.

Without a home system, the mapper can't tell a system's parent from its children, so the parent's number counts as taken and a first hole may be suggested as `2`.

## Example

Home has two holes, `1` and `2`. Inside the `1` there are two more:

```
Home
├── 1   (C5)
│   ├── 1   (C6)
│   └── 2   (C4)
└── 2   (C5)
```

Standing in the C6, the bookmark back to its parent reads `*1 …`: it's a return, so it gets the return format even though both are called `1`.
