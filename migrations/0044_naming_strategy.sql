-- Which naming convention a map follows. `chain` is the one every map had before there
-- was a choice: an alias extends its parent's. `siblings` numbers each system's holes
-- afresh, 1, 2, 3, without the parent's alias in front.
create type naming_strategy as enum ('chain', 'siblings');

alter table maps add column naming_strategy naming_strategy not null default 'chain';
