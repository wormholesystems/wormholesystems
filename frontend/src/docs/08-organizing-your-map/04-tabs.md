---
title: Tabs
---

# Tabs

A big map gets crowded: the home chain, thirty exits out of Thera, another thirty out of Turnur. **Tabs** split a map over several canvases. Each tab works like a map of its own, and very little ties them together.

## Each tab is its own canvas

Everything you do on a tab stays on that tab:

- Systems and connections are placed, moved and removed per tab. The same system can be on two tabs, as two separate nodes.
- **Alias, pin, home and rally** are set per tab. A system can be "Hub" on one tab and unnamed on another.
- **Signatures** are per tab too. Scan Jita on one tab and the other tab's Jita knows nothing of it, and an unmapped hole is drawn on the tab its signature was scanned on.
- **Clear map**, **Clean map** and the stale connections sweep only touch the tab you are looking at.
- **Undo and redo** walk the changes made on this tab, so undoing here never reverts what a corpmate just did on another.

The tab you are on is part of the address, so a reload or a shared link opens on the same tab.

## What the tabs share

- **Access and settings.** Whoever can see the map sees all its tabs, with the same role. Your own settings, the watchlist and the ignore list are per map, not per tab.
- **Routes.** [Routing](/documentation/autopilot-and-routing/how-routing-works) uses the connections of every tab, so a route can run through a hole that is drawn on another one.
- **Intel.** A system's status, occupier and notes are the same on every tab: set them on one and they show on the others.

## Seeing where tabs meet

When a system on this tab is also on another one, a small link under the node names the other tab. Click it to go there with that system selected.

## Managing tabs

Tabs are switched on per map by whoever runs the site. On a map without them there is no tab bar.

The tabs sit next to the map name. Managers and owners can add a tab with **+**, rename one by double-clicking it, and reorder or delete from the menu on the active tab.

Deleting a tab deletes everything drawn on it, including its history, and cannot be undone. The other tabs are not touched. The first tab is the map itself and cannot be deleted.
