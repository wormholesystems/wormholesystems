-- Keep only the SHA-256 of the session id, the way personal access tokens are kept, so a
-- copy of this table is not a copy of everyone's cookies. The plaintext ids already stored
-- cannot be turned into hashes we would trust, so every session ends and everyone signs in
-- once more.
delete from sessions;
alter table sessions rename column id to id_hash;
