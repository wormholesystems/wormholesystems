-- The unclaimed-jump prune runs every five minutes on `connection_id is null and created_at <
-- ..`, which neither jump index matches. Claimed jumps are the bulk of the table, so the
-- index only holds the pending ones.
create index map_connection_jumps_unclaimed
    on map_connection_jumps (created_at) where connection_id is null;
