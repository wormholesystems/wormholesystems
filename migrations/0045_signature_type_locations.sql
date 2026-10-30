-- Where a signature type spawns, beyond the system classes in its spawn areas. Rows name a
-- region or constellation of known space (pirate sites go with their faction's regions,
-- some nebulae with one constellation), or a single system the type is named for.
--
-- A type without region or constellation rows spawns wherever its spawn areas allow; with
-- them, a known-space system must be in one. Wormhole space ignores those, so pirate sites
-- in C1-C3 stay unrestricted. A system row replaces rather than narrows: where any type of
-- a category names a system, only those types of that category spawn there, which is how
-- the Sansha-occupied shattered wormholes trade their Sleeper sites for Sansha ones.
create table signature_type_locations (
    signature_type_id bigint not null references signature_types (id) on delete cascade,
    region_id         bigint references regions (id),
    constellation_id  bigint references constellations (id),
    solar_system_id   bigint references solar_systems (id),
    check (num_nonnulls(region_id, constellation_id, solar_system_id) = 1)
);

create unique index signature_type_locations_region
    on signature_type_locations (signature_type_id, region_id) where region_id is not null;
create unique index signature_type_locations_constellation
    on signature_type_locations (signature_type_id, constellation_id)
    where constellation_id is not null;
create unique index signature_type_locations_system
    on signature_type_locations (signature_type_id, solar_system_id)
    where solar_system_id is not null;
