-- Docking is read straight from ESI's location. A structure is named only once a pilot with
-- the optional structures permission docks in it, so most are not in `structures` when a
-- pilot is first seen inside. The ids say "docked" on their own; the name is a left join.
alter table character_status
    drop constraint character_status_station_id_fkey,
    drop constraint character_status_structure_id_fkey;
