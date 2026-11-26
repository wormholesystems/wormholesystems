-- Tabs: a map split over several canvases, each of which is a map in its own right.
--
-- A tab is a row in `maps` that names another as its parent. Everything drawn on it (its
-- systems, connections, signatures and history) hangs off its own id, exactly as on any
-- map, so a tab is edited, cleaned and undone on its own and nothing that works on one
-- map had to learn about several. What a tab does not have is anything about who may see
-- it or how they like it shown: grants, sharing, the watchlist, the ignore list and each
-- viewer's settings stay with the parent, and are looked up through it.
--
-- The parent is itself the first tab, so a map nobody has split is one row, as before.
alter table maps
    add column parent_id bigint references maps (id) on delete cascade,
    -- What the tab is called in the bar. A child's `name` is its parent's, kept in step,
    -- so anything that prints a map's name still says which map.
    add column tab_name text not null default 'Main',
    -- Left to right in the bar; ties fall back to the id.
    add column tab_order int not null default 0;

create index maps_parent on maps (parent_id) where parent_id is not null;

-- One level only: a tab has no tabs of its own.
create function maps_tab_is_flat() returns trigger language plpgsql as $$
begin
    if new.parent_id is not null
       and exists (select 1 from maps p where p.id = new.parent_id and p.parent_id is not null)
    then
        raise exception 'a tab cannot have tabs of its own';
    end if;
    return new;
end $$;

create trigger maps_tab_is_flat before insert or update of parent_id on maps
    for each row execute function maps_tab_is_flat();

-- The map a row belongs to: itself, or its parent when it is a tab.
create function map_root(bigint) returns bigint language sql stable as $$
    select coalesce(parent_id, id) from maps where id = $1
$$;

-- A grant on a map is a grant on each of its tabs. Said here, in the view every role
-- check already reads, so no check has to know a tab from a map.
create or replace view map_access_live as
    select a.id, m.id as map_id, a.subject_type, a.subject_id, a.role, a.expires_at,
           a.created_at
    from map_access a
    join maps m on coalesce(m.parent_id, m.id) = a.map_id
    where a.expires_at is null or a.expires_at > now();

-- A tab is shown the way its map is: the naming, the placement mode and whether unmapped
-- holes are drawn are the map's settings, read off each tab's own row by everything that
-- works on one map. Kept in step here, so no writer of those settings has to remember the
-- tabs, and a new tab starts as its map already is.
create function maps_tab_takes_settings() returns trigger language plpgsql as $$
begin
    select p.name, p.description, p.image_url, p.alias_scheme, p.naming_strategy,
           p.ignored_alias, p.bookmark_wormhole, p.bookmark_kspace, p.bookmark_return,
           p.ghost_unlinked_wormholes, p.layout, p.allow_layout_override
    into new.name, new.description, new.image_url, new.alias_scheme, new.naming_strategy,
         new.ignored_alias, new.bookmark_wormhole, new.bookmark_kspace, new.bookmark_return,
         new.ghost_unlinked_wormholes, new.layout, new.allow_layout_override
    from maps p where p.id = new.parent_id;
    return new;
end $$;

create trigger maps_tab_takes_settings before insert on maps
    for each row when (new.parent_id is not null)
    execute function maps_tab_takes_settings();

create function maps_settings_reach_tabs() returns trigger language plpgsql as $$
begin
    update maps t
    set name = new.name, description = new.description, image_url = new.image_url,
        alias_scheme = new.alias_scheme, naming_strategy = new.naming_strategy,
        ignored_alias = new.ignored_alias, bookmark_wormhole = new.bookmark_wormhole,
        bookmark_kspace = new.bookmark_kspace, bookmark_return = new.bookmark_return,
        ghost_unlinked_wormholes = new.ghost_unlinked_wormholes, layout = new.layout,
        allow_layout_override = new.allow_layout_override
    where t.parent_id = new.id;
    return new;
end $$;

create trigger maps_settings_reach_tabs after update on maps
    for each row when (new.parent_id is null)
    execute function maps_settings_reach_tabs();
